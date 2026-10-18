#!/usr/bin/env python3
"""
DSports' private, on-device SoulSilver APK builder.

Runs inside DSports' embedded Android build environment. The ROM is opened from private local storage and
is never uploaded. The upstream SSRecomp tools generate game-specific C++ locally;
the Android runtime, SDL2, and the resulting APK are built on-device.
"""
import argparse
import importlib.util
import os
import shutil
import subprocess
import sys
import time
import traceback
from pathlib import Path

TAG = "v0.3.0"
REPO = "https://github.com/ClickingMaster/ssrecomp.git"


def parse_args():
    ap = argparse.ArgumentParser()
    ap.add_argument("--rom", required=True)
    ap.add_argument("--status", required=True)
    ap.add_argument("--output", required=True)
    ap.add_argument("--jobs", type=int, default=2)
    return ap.parse_args()


ARGS = parse_args()
STATUS_LINES = []
LAST_STATUS_WRITE = 0.0
MAX_STATUS_LINES = 80


def write_status(line, force=False):
    global LAST_STATUS_WRITE
    line = str(line).replace("\x00", "")
    print(line, flush=True)
    STATUS_LINES.append(line)
    del STATUS_LINES[:-MAX_STATUS_LINES]
    now = time.monotonic()
    if force or now - LAST_STATUS_WRITE > 0.25 or line.startswith("BUILD_"):
        try:
            os.makedirs(os.path.dirname(ARGS.status), exist_ok=True)
            with open(ARGS.status, "w", encoding="utf-8") as f:
                f.write("\n".join(STATUS_LINES) + "\n")
                f.flush()
            LAST_STATUS_WRITE = now
        except OSError:
            pass


def log(line):
    write_status(line)


def progress(fraction, label):
    if fraction is None:
        write_status("STATUS | " + label)
    else:
        write_status("PROGRESS {:3d}% | {}".format(max(0, min(100, int(fraction * 100))), label))


def run_checked(cmd, cwd=None):
    write_status("$ " + " ".join(str(x) for x in cmd))
    subprocess.run(cmd, cwd=cwd, check=True)


def load_setup(repo_dir):
    setup_path = repo_dir / "ssrecomp_setup.py"
    spec = importlib.util.spec_from_file_location("dsports_ssrecomp_setup", str(setup_path))
    if spec is None or spec.loader is None:
        raise RuntimeError("Could not load the SSRecomp setup module.")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def create_embedded_android_steps(setup):
    """Return the upstream Android build step with the app's embedded Android-native host tools.

    Google's NDK supplies Android headers and libraries; The embedded clang/LLVM binaries
    run natively on this ARM64 Android device. Linux-host NDK compiler executables
    are deliberately not invoked on Android.
    """
    def android_steps(rom, jobs, log_fn, progress_fn, run):
        prefix = os.environ.get("PREFIX", "/data/data/com.dsprtf/files/usr")
        bindir = os.path.join(prefix, "bin")
        required = ["clang", "clang++", "llvm-ar", "llvm-ranlib", "ld.lld", "llvm-strip"]
        missing = [name for name in required if not os.path.exists(os.path.join(bindir, name))]
        if missing:
            raise RuntimeError(
                "Missing embedded compiler tools: " + ", ".join(missing) +
                ". The embedded runtime package installation did not complete; retry the build."
            )

        ndk = setup.android_ndk(log_fn, progress_fn)
        toolchain = os.path.join(ndk, "build", "cmake", "android.toolchain.cmake").replace("\\", "/")
        api = setup.ANDROID_PLATFORM.split("-")[-1]
        target = "aarch64-linux-android" + api
        tool_overrides = [
            "-DCMAKE_C_COMPILER=" + os.path.join(bindir, "clang"),
            "-DCMAKE_CXX_COMPILER=" + os.path.join(bindir, "clang++"),
            "-DCMAKE_ASM_COMPILER=" + os.path.join(bindir, "clang"),
            "-DCMAKE_C_COMPILER_TARGET=" + target,
            "-DCMAKE_CXX_COMPILER_TARGET=" + target,
            "-DCMAKE_ASM_COMPILER_TARGET=" + target,
            "-DCMAKE_AR=" + os.path.join(bindir, "llvm-ar"),
            "-DCMAKE_RANLIB=" + os.path.join(bindir, "llvm-ranlib"),
            "-DCMAKE_LINKER=" + os.path.join(bindir, "ld.lld"),
            "-DCMAKE_STRIP=" + os.path.join(bindir, "llvm-strip"),
        ]
        android_args = [
            "-G", "Ninja",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DCMAKE_TOOLCHAIN_FILE=" + toolchain,
            "-DANDROID_ABI=" + setup.ANDROID_ABI,
            "-DANDROID_PLATFORM=" + setup.ANDROID_PLATFORM,
            *tool_overrides,
        ]

        # Build SDL2 locally for Android, rather than using a host-only SDL2 library.
        sdl_prefix = os.path.join(setup.ANDROID_DIR, "sdl2-" + setup.ANDROID_ABI)
        sdl_lib = os.path.join(sdl_prefix, "lib", "libSDL2.so")
        if not os.path.exists(sdl_lib):
            os.makedirs(os.path.join(setup.ANDROID_DIR, "downloads"), exist_ok=True)
            tar_path = os.path.join(setup.ANDROID_DIR, "downloads", setup.SDL2_NAME + ".tar.gz")
            setup.download(
                setup.SDL2_DOWNLOAD[0], setup.SDL2_DOWNLOAD[1], tar_path,
                log_fn, progress_fn, "Downloading SDL2 source", "sha256"
            )
            import tarfile
            with tarfile.open(tar_path) as archive:
                archive.extractall(setup.ANDROID_DIR)
            progress_fn(0.10, "Configuring Android SDL2 with the on-device compiler")
            sdl_build = os.path.join(setup.ANDROID_DIR, "build-sdl2-" + setup.ANDROID_ABI)
            run([
                "cmake", "-S", os.path.join(setup.ANDROID_DIR, setup.SDL2_NAME),
                "-B", sdl_build, *android_args,
                "-DCMAKE_INSTALL_PREFIX=" + sdl_prefix.replace("\\", "/"),
                "-DSDL_SHARED=ON", "-DSDL_STATIC=OFF", "-DSDL_TEST=OFF"
            ])
            run(["ninja", "-C", sdl_build, "-j", str(jobs), "install"])

        build = os.path.join(setup.HERE, "build-android-" + setup.ANDROID_ABI)
        progress_fn(0.12, "Configuring the SoulSilver Android runtime")
        setup.forget_moved_build(build)
        if not os.path.exists(os.path.join(build, "build.ninja")):
            sdl_slash = sdl_prefix.replace("\\", "/")
            run([
                "cmake", "-S", setup.HERE, "-B", build, *android_args,
                "-DSDL2_DIR=" + sdl_slash + "/lib/cmake/SDL2",
                "-DSSR_SDL2_HEADERS=" + sdl_slash + "/include"
            ])
        progress_fn(0.14, "Compiling the port for Android; this can take a long time")
        run(["ninja", "-C", build, "-j", str(jobs)], parse_ninja=True)

        progress_fn(0.98, "Packaging and signing the local APK")
        sys.path.insert(0, os.path.join(setup.HERE, "tools", "android"))
        import apk
        apk.build(
            os.path.join(setup.HERE, "tools", "android", "skeleton.apk"),
            setup.ANDROID_ABI,
            os.path.join(build, "libmain.so"),
            sdl_lib,
            rom,
            os.path.join(setup.ANDROID_DIR, "signing-key.json"),
            setup.ANDROID_APK,
        )
        setup.write_rom_note(rom)
        os.makedirs(os.path.dirname(ARGS.output), exist_ok=True)
        shutil.copy2(setup.ANDROID_APK, ARGS.output)
        progress_fn(1.0, "APK written to " + ARGS.output)
    return android_steps


def main():
    rom = os.path.abspath(ARGS.rom)
    out = os.path.abspath(ARGS.output)
    status = os.path.abspath(ARGS.status)
    if not os.path.isfile(rom):
        raise RuntimeError("The selected ROM copy cannot be found at " + rom)
    if os.path.getsize(rom) < 1024 * 1024:
        raise RuntimeError("The selected file is too small to be a Nintendo DS ROM.")

    root = Path.home() / ".local" / "share" / "dsports" / ("ssrecomp-" + TAG)
    root.parent.mkdir(parents=True, exist_ok=True)
    write_status("DSports standalone on-device native port builder", True)
    write_status("ROM stays on this device. It is not uploaded to GitHub or any build server.", True)
    write_status("Build workspace: " + str(root), True)
    if not (root / ".git").exists():
        if root.exists():
            shutil.rmtree(root)
        run_checked(["git", "clone", "--depth", "1", "--branch", TAG, REPO, str(root)])
    setup = load_setup(root)
    setup.android_steps = create_embedded_android_steps(setup)

    # Two jobs are the conservative default for handhelds; the caller can select more.
    jobs = max(1, min(8, ARGS.jobs))
    write_status("Checking local ROM and generating native game code.", True)
    try:
        setup.run_steps(rom, jobs, log, progress, android=True)
        # The upstream build writes into its workspace; our custom Android step copies
        # the resulting APK to the output path supplied by DSports.
        if not os.path.isfile(out) or os.path.getsize(out) < 100_000:
            raise RuntimeError("Build ended without a valid APK at the requested output path.")
        write_status("BUILD_SUCCEEDED", True)
        write_status("PORT_APK=" + out, True)
        write_status("Your port APK includes the locally supplied game. Keep it private.", True)
    except Exception as exc:
        write_status("BUILD_FAILED", True)
        write_status(type(exc).__name__ + ": " + str(exc), True)
        traceback.print_exc()
        raise


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        write_status("BUILD_FAILED", True)
        write_status("ERROR: " + str(exc), True)
        sys.exit(1)
