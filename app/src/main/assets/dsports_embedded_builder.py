#!/usr/bin/env python3
"""
DSports' private, on-device SoulSilver APK builder.

Runs inside DSports' embedded Android build environment. The ROM is opened from
private local storage and is never uploaded. The upstream SSRecomp tools generate
game-specific C++ locally; the Android runtime, SDL2 and the resulting APK are
built on the device itself.

The parts that do not need a device (argument parsing, the NDK toolchain patch,
the display-default patch and APK verification) are plain functions so that
`tools/test_embedded_builder.py` can exercise them on a normal PC.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import traceback
import zipfile
from pathlib import Path

TAG = "v0.4.2"
REPO = "https://github.com/ClickingMaster/ssrecomp.git"

# Layouts offered by DSports. The first one is the default.
LAYOUTS = ("merged", "side", "vertical", "hybrid", "top", "bottom")
# The most lines the widescreen 3D world is drawn at; "native" is the DS' own 192.
RESOLUTIONS = {"native": 192, "540": 540, "720": 720, "1080": 1080, "1440": 1440}
DEFAULT_LAYOUT = "merged"
DEFAULT_RESOLUTION = "720"

# Files the finished APK must contain, with the reason for each.
REQUIRED_APK_ENTRIES = (
    "AndroidManifest.xml",
    "classes.dex",
    "resources.arsc",
    "assets/rom.nds",
)
REQUIRED_APK_LIBS = ("libmain.so", "libSDL2.so")

STATUS_LINES = []
LAST_STATUS_WRITE = 0.0
MAX_STATUS_LINES = 80
STATUS_PATH = None


# ---------------------------------------------------------------------------
# Logging. The status file is what the Android UI polls.
def set_status_path(path):
    global STATUS_PATH
    STATUS_PATH = str(path) if path else None


def write_status(line, force=False):
    global LAST_STATUS_WRITE
    line = str(line).replace("\x00", "")
    print(line, flush=True)
    STATUS_LINES.append(line)
    del STATUS_LINES[:-MAX_STATUS_LINES]
    now = time.monotonic()
    if STATUS_PATH and (force or now - LAST_STATUS_WRITE > 0.25 or line.startswith("BUILD_")):
        try:
            os.makedirs(os.path.dirname(STATUS_PATH), exist_ok=True)
            with open(STATUS_PATH, "w", encoding="utf-8") as f:
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


# ---------------------------------------------------------------------------
# Arguments. Nothing is parsed at import time so the module can be tested.

def parse_args(argv=None):
    ap = argparse.ArgumentParser(description="DSports on-device APK builder")
    ap.add_argument("--rom", required=True)
    ap.add_argument("--status", required=True)
    ap.add_argument("--output", required=True)
    ap.add_argument("--jobs", type=int, default=2)
    ap.add_argument("--layout", default=DEFAULT_LAYOUT, choices=LAYOUTS)
    ap.add_argument("--resolution", default=DEFAULT_RESOLUTION, choices=tuple(RESOLUTIONS))
    ap.add_argument("--smooth", action="store_true", help="filtered instead of nearest-pixel scaling")
    ap.add_argument("--widescreen", dest="widescreen", action="store_true", default=True)
    ap.add_argument("--no-widescreen", dest="widescreen", action="store_false")
    ap.add_argument("--windowed", action="store_true", help="start windowed instead of fullscreen")
    ap.add_argument("--self-test", action="store_true", help=argparse.SUPPRESS)
    args = ap.parse_args(argv)
    args.jobs = max(1, min(8, args.jobs))
    args.wide_lines = RESOLUTIONS[args.resolution]
    return args


# ---------------------------------------------------------------------------
# NDK toolchain patch.

def _toolchain_files(ndk_dir):
    """Both NDK toolchain files, the one that is used and the one it includes."""
    base = Path(ndk_dir) / "build" / "cmake"
    return [p for p in (base / "android-legacy.toolchain.cmake", base / "android.toolchain.cmake") if p.is_file()]


def _replace_assignment(lines, variable, replacement):
    """Replaces the whole `set(<variable> ...)` statement, which may span lines."""
    pattern = re.compile(r"^\s*set\(\s*" + re.escape(variable) + r"(?=$|[\s)])")
    found = None
    for index, line in enumerate(lines):
        if pattern.match(line):
            found = index
            break
    if found is None:
        return False
    end = found
    while end < len(lines) and ")" not in lines[end]:
        end += 1
    if end >= len(lines):
        raise RuntimeError("Unterminated NDK toolchain assignment for " + variable + ".")
    if lines[found:end + 1] == replacement:
        return False
    lines[found:end + 1] = replacement
    return True


def patch_ndk_toolchain(ndk_dir, log_fn=None, prefix=None):
    """Point the NDK's toolchain at Android-native host tools.

    The NDK bundle is a Linux/x86_64 download: its `clang`, `llvm-ar` and friends
    cannot execute on an ARM64 Android device. Only its headers, crt objects and
    per-architecture libraries are used; the compilers and binutils come from the
    embedded ARM64 toolchain. Returns the list of patched files.
    """
    log_fn = log_fn or (lambda _line: None)
    prefix = prefix or os.environ.get("PREFIX", "/data/data/com.dsprtm/files/usr")
    bindir = os.path.join(prefix, "bin")
    tools = {
        "ANDROID_C_COMPILER": "clang",
        "ANDROID_CXX_COMPILER": "clang++",
        "ANDROID_ASM_COMPILER": "clang",
        "ANDROID_AR": "llvm-ar",
        "ANDROID_RANLIB": "llvm-ranlib",
        "ANDROID_STRIP": "llvm-strip",
    }
    files = _toolchain_files(ndk_dir)
    if not files:
        raise RuntimeError(
            "The Android NDK is missing build/cmake/android-legacy.toolchain.cmake; "
            "cannot configure the on-device ARM64 compiler."
        )

    patched = []
    for path in files:
        text = path.read_text(encoding="utf-8")
        lines = text.splitlines()
        changed = False
        for variable, executable in tools.items():
            replacement = ['set(' + variable + ' "$ENV{PREFIX}/bin/' + executable + '")']
            changed |= _replace_assignment(lines, variable, replacement)
        # `-static-libstdc++` is a GCC flag: clang rejects it and the link fails.
        # Android's clang already links the NDK's libc++_static.a by default.
        joined = "\n".join(lines) + "\n"
        if "-static-libstdc++" in joined:
            joined = joined.replace("  list(APPEND ANDROID_LINKER_FLAGS \"-static-libstdc++\")", "")
            joined = joined.replace('list(APPEND ANDROID_LINKER_FLAGS "-static-libstdc++")', "")
            changed = True
        if changed:
            path.write_text(joined, encoding="utf-8")
            patched.append(str(path))

    if not patched:
        # Already patched by an earlier build of this checkout: verify it held.
        text = files[0].read_text(encoding="utf-8")
        if "$ENV{PREFIX}/bin/clang" not in text:
            raise RuntimeError("Could not patch the NDK toolchain file at " + str(files[0]) + ".")

    for name in tools.values():
        candidate = os.path.join(bindir, name)
        if not os.path.exists(candidate):
            raise RuntimeError(
                "The embedded compiler tool '" + name + "' is missing from " + bindir +
                ". The embedded toolchain installation is incomplete; retry the build."
            )
    log_fn("Configured the NDK toolchain to use the device's own ARM64 compiler and LLVM tools.")
    return patched


def toolchain_sysroot(ndk_dir, host_tag="linux-x86_64"):
    """The NDK sysroot: architecture-neutral headers plus per-ABI libraries."""
    return os.path.join(str(ndk_dir), "toolchains", "llvm", "prebuilt", host_tag, "sysroot")


def preflight_toolchain(ndk_dir, work_dir, log_fn=None):
    """Compile and link a tiny library before the real build starts.

    A wrong sysroot, a missing crt object or a compiler that cannot run turns into
    a clear message here instead of a wall of Ninja output twenty minutes later.
    """
    log_fn = log_fn or (lambda _line: None)
    sysroot = toolchain_sysroot(ndk_dir)
    if not os.path.isdir(sysroot):
        raise RuntimeError("The Android NDK sysroot is missing at " + sysroot + ".")
    prefix = os.environ.get("PREFIX", "/data/data/com.dsprtm/files/usr")
    bindir = os.path.join(prefix, "bin")
    target = "aarch64-linux-android24"
    os.makedirs(work_dir, exist_ok=True)

    samples = (
        ("clang", "preflight.c", "int dsports_preflight(void) { return 42; }\n"),
        ("clang++", "preflight.cpp",
         "#include <string>\n"
         "int dsports_preflight_cpp() { std::string s(\"dsports\"); return (int)s.size(); }\n"),
    )
    for compiler, name, source in samples:
        source_path = os.path.join(work_dir, name)
        with open(source_path, "w", encoding="utf-8") as f:
            f.write(source)
        out = os.path.join(work_dir, "libpreflight_" + compiler.replace("+", "p") + ".so")
        cmd = [
            os.path.join(bindir, compiler),
            "--target=" + target,
            "--sysroot=" + sysroot,
            "-std=c++17" if compiler == "clang++" else "-std=c11",
            "-fPIC", "-shared", "-o", out, source_path,
        ]
        log_fn("$ " + " ".join(cmd))
        result = subprocess.run(cmd, capture_output=True, text=True)
        if result.returncode != 0:
            detail = (result.stderr or result.stdout or "").strip()
            raise RuntimeError(
                "The embedded " + compiler + " cannot build for Android yet:\n" + detail[-1500:]
            )
        with open(out, "rb") as f:
            header = f.read(20)
        if header[:4] != b"\x7fELF" or header[18:20] != b"\xb7\x00":
            raise RuntimeError(
                "The preflight library is not an ARM64 Android shared object (" + out + ")."
            )
    log_fn("Verified the embedded ARM64 compiler, linker and C++ standard library.")


# ---------------------------------------------------------------------------
# Build-time display defaults, patched into the frontend's settings struct.

DEFAULT_PATCHES = (
    # (regex, replacement builder, description)
    (r'(\bstd::string\s+layout\s*=\s*")([a-z]+)(";)', "layout", "view layout"),
    (r'(\bint\s+wideLines\s*=\s*)(\d+)(;)', "wide_lines", "world detail"),
    (r'(\bbool\s+smooth\s*=\s*)(true|false)(;)', "smooth", "smooth scaling"),
    (r'(\bbool\s+wide\s*=\s*)(true|false)(;)', "wide", "widescreen world"),
    (r'(\bbool\s+fullscreen\s*=\s*)(true|false)(;)', "fullscreen", "fullscreen"),
)


def _cpp_bool(value):
    return "true" if value else "false"


def patch_frontend_defaults(root, options, log_fn=None):
    """Make the port start with the display settings chosen in DSports.

    The pause menu inside the port keeps offering all of them, so these are only
    defaults. Every pattern must match exactly once; a silent no-op would ship a
    port that ignores the user's choice, which is worse than a failed build.
    """
    log_fn = log_fn or (lambda _line: None)
    path = Path(root) / "frontend" / "main.cpp"
    if not path.is_file():
        raise RuntimeError("The SSRecomp checkout has no frontend/main.cpp at " + str(path) + ".")

    text = path.read_text(encoding="utf-8")
    values = {
        "layout": options.layout,
        "wide_lines": str(options.wide_lines),
        "smooth": _cpp_bool(options.smooth),
        "wide": _cpp_bool(options.widescreen),
        "fullscreen": _cpp_bool(not options.windowed),
    }

    changed = []
    for pattern, key, label in DEFAULT_PATCHES:
        matches = list(re.finditer(pattern, text))
        if len(matches) != 1:
            raise RuntimeError(
                "Could not locate the " + label + " default in frontend/main.cpp "
                "(found " + str(len(matches)) + " matches); the upstream source layout changed."
            )
        match = matches[0]
        wanted = match.group(1) + values[key] + match.group(3)
        if match.group(0) != wanted:
            text = text[:match.start()] + wanted + text[match.end():]
            changed.append(label)

    if changed:
        path.write_text(text, encoding="utf-8")
        log_fn("Set the port's display defaults: " + ", ".join(changed) + ".")
    else:
        log_fn("The port's display defaults already match this build.")
    return changed


# ---------------------------------------------------------------------------
# APK verification.

def verify_apk(path, min_bytes=100_000):
    """Checks that the file is a complete, signed APK with the game and both libs."""
    if not os.path.isfile(path):
        raise RuntimeError("No APK was produced at " + str(path) + ".")
    size = os.path.getsize(path)
    if size < min_bytes:
        raise RuntimeError("The produced APK is only " + str(size) + " bytes; it is incomplete.")

    try:
        with zipfile.ZipFile(path) as apk:
            names = set(apk.namelist())
            bad = apk.testzip()
    except (OSError, zipfile.BadZipFile) as exc:
        raise RuntimeError("The produced APK is not a valid archive: " + str(exc))
    if bad:
        raise RuntimeError("The produced APK is corrupt at entry " + bad + ".")

    missing = [name for name in REQUIRED_APK_ENTRIES if name not in names]
    missing += [name for name in REQUIRED_APK_LIBS if "lib/arm64-v8a/" + name not in names]
    if missing:
        raise RuntimeError("The produced APK is missing: " + ", ".join(missing) + ".")

    with open(path, "rb") as f:
        head = f.read(4)
        # A ZIP central-directory record ends the file (optionally after a comment).
        f.seek(max(0, size - 66_000))
        tail = f.read()
    if head[:2] != b"PK" or b"PK\x05\x06" not in tail:
        raise RuntimeError("The produced APK is not a ZIP file.")

    signature = _apk_v2_signature_present(path)
    if not signature:
        raise RuntimeError(
            "The produced APK is unsigned (no APK Signature Scheme v2 block); "
            "Android would refuse to install it."
        )
    return {"size": size, "entries": len(names)}


def _apk_v2_signature_present(path, tail_bytes=8 * 1024 * 1024):
    """The APK signing block ends just before the central directory.

    Only the tail of the file is read: a game APK can be hundreds of megabytes and
    loading it whole on a handheld would cost as much memory as the ROM itself.
    """
    size = os.path.getsize(path)
    start = max(0, size - tail_bytes)
    with open(path, "rb") as f:
        f.seek(start)
        data = f.read()
    marker = b"APK Sig Block 42"
    position = data.rfind(marker)
    if position < 0:
        return False
    eocd = data.rfind(b"PK\x05\x06")
    return eocd > position


# ---------------------------------------------------------------------------
# The build.

def load_setup(repo_dir):
    import importlib.util

    setup_path = Path(repo_dir) / "ssrecomp_setup.py"
    spec = importlib.util.spec_from_file_location("dsports_ssrecomp_setup", str(setup_path))
    if spec is None or spec.loader is None:
        raise RuntimeError("Could not load the SSRecomp setup module.")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def create_embedded_android_steps(setup, options, args_output):
    """The upstream Android step, built with this device's own compilers.

    Google's NDK supplies the Android headers, crt objects and per-ABI libraries;
    the embedded ARM64 clang/LLVM build the game. The NDK's Linux/x86_64 host
    binaries are never executed.
    """

    def android_steps(rom, jobs, log_fn, progress_fn, run):
        ndk = setup.android_ndk(log_fn, progress_fn)
        patch_ndk_toolchain(ndk, log_fn)
        preflight_toolchain(ndk, os.path.join(setup.ANDROID_DIR, "preflight"), log_fn)

        toolchain = os.path.join(ndk, "build", "cmake", "android.toolchain.cmake").replace("\\", "/")
        sysroot = toolchain_sysroot(ndk).replace("\\", "/")
        target = "aarch64-linux-android" + setup.ANDROID_PLATFORM.split("-")[-1]
        prefix = os.environ.get("PREFIX", "/data/data/com.dsprtm/files/usr")
        bindir = os.path.join(prefix, "bin")

        tool_overrides = [
            "-DCMAKE_C_COMPILER=" + os.path.join(bindir, "clang"),
            "-DCMAKE_CXX_COMPILER=" + os.path.join(bindir, "clang++"),
            "-DCMAKE_ASM_COMPILER=" + os.path.join(bindir, "clang"),
            "-DCMAKE_C_COMPILER_TARGET=" + target,
            "-DCMAKE_CXX_COMPILER_TARGET=" + target,
            "-DCMAKE_ASM_COMPILER_TARGET=" + target,
            "-DCMAKE_AR=" + os.path.join(bindir, "llvm-ar"),
            "-DCMAKE_RANLIB=" + os.path.join(bindir, "llvm-ranlib"),
            "-DCMAKE_STRIP=" + os.path.join(bindir, "llvm-strip"),
            "-DCMAKE_SYSROOT=" + sysroot,
            # SDL2 lives outside the NDK, so package lookup may not be restricted to it.
            "-DCMAKE_FIND_ROOT_PATH_MODE_PACKAGE=BOTH",
            "-DANDROID_USE_LEGACY_TOOLCHAIN_FILE=ON",
            "-DANDROID_STL=c++_static",
        ]
        android_args = [
            "-G", "Ninja",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DCMAKE_TOOLCHAIN_FILE=" + toolchain,
            "-DANDROID_ABI=" + setup.ANDROID_ABI,
            "-DANDROID_PLATFORM=" + setup.ANDROID_PLATFORM,
            *tool_overrides,
        ]

        # SDL2, built for Android with the same compilers.
        sdl_prefix = os.path.join(setup.ANDROID_DIR, "sdl2-" + setup.ANDROID_ABI)
        sdl_lib = os.path.join(sdl_prefix, "lib", "libSDL2.so")
        if not os.path.exists(sdl_lib):
            os.makedirs(os.path.join(setup.ANDROID_DIR, "downloads"), exist_ok=True)
            tar_path = os.path.join(setup.ANDROID_DIR, "downloads", setup.SDL2_NAME + ".tar.gz")
            setup.download(
                setup.SDL2_DOWNLOAD[0], setup.SDL2_DOWNLOAD[1], tar_path,
                log_fn, progress_fn, "Downloading SDL2 source", "sha256"
            )
            with tarfile.open(tar_path) as archive:
                archive.extractall(setup.ANDROID_DIR)
            progress_fn(0.10, "Configuring Android SDL2 with the on-device compiler")
            sdl_build = os.path.join(setup.ANDROID_DIR, "build-sdl2-" + setup.ANDROID_ABI)
            run([
                "cmake", "-S", os.path.join(setup.ANDROID_DIR, setup.SDL2_NAME),
                "-B", sdl_build, *android_args,
                "-DCMAKE_INSTALL_PREFIX=" + sdl_prefix.replace("\\", "/"),
                "-DSDL_SHARED=ON", "-DSDL_STATIC=OFF", "-DSDL_TEST=OFF",
            ])
            run(["ninja", "-C", sdl_build, "-j", str(jobs), "install"])
        if not os.path.exists(sdl_lib):
            raise RuntimeError("SDL2 did not produce libSDL2.so at " + sdl_lib + ".")

        # The game, as libmain.so.
        build = os.path.join(setup.HERE, "build-android-" + setup.ANDROID_ABI)
        progress_fn(0.12, "Configuring the SoulSilver Android runtime")
        setup.forget_moved_build(build)
        if not os.path.exists(os.path.join(build, "build.ninja")):
            sdl_slash = sdl_prefix.replace("\\", "/")
            run([
                "cmake", "-S", setup.HERE, "-B", build, *android_args,
                "-DSDL2_DIR=" + sdl_slash + "/lib/cmake/SDL2",
                "-DSSR_SDL2_HEADERS=" + sdl_slash + "/include",
            ])
        progress_fn(0.14, "Compiling the port for Android; this can take a long time")
        run(["ninja", "-C", build, "-j", str(jobs)], parse_ninja=True)

        libmain = os.path.join(build, "libmain.so")
        if not os.path.isfile(libmain):
            raise RuntimeError("The build finished without producing libmain.so.")

        progress_fn(0.98, "Packaging and signing the local APK")
        sys.path.insert(0, os.path.join(setup.HERE, "tools", "android"))
        import apk
        apk.build(
            os.path.join(setup.HERE, "tools", "android", "skeleton.apk"),
            setup.ANDROID_ABI,
            libmain,
            sdl_lib,
            rom,
            os.path.join(setup.ANDROID_DIR, "signing-key.json"),
            setup.ANDROID_APK,
        )
        setup.write_rom_note(rom)
        os.makedirs(os.path.dirname(args_output), exist_ok=True)
        shutil.copy2(setup.ANDROID_APK, args_output)
        verify_apk(args_output)
        progress_fn(1.0, "APK written to " + args_output)

    return android_steps


def checkout(repo_dir):
    """A shallow checkout of the pinned tag, reused between builds."""
    root = Path(repo_dir)
    if not (root / ".git").exists():
        if root.exists():
            shutil.rmtree(root)
        root.parent.mkdir(parents=True, exist_ok=True)
        run_checked(["git", "clone", "--depth", "1", "--branch", TAG, REPO, str(root)])
    return root


def main(argv=None):
    args = parse_args(argv)
    if args.self_test:
        print("OK: builder module imports and parses arguments.")
        return 0
    set_status_path(args.status)

    rom = os.path.abspath(args.rom)
    out = os.path.abspath(args.output)
    if not os.path.isfile(rom):
        raise RuntimeError("The selected ROM copy cannot be found at " + rom)
    if os.path.getsize(rom) < 1024 * 1024:
        raise RuntimeError("The selected file is too small to be a Nintendo DS ROM.")

    write_status("DSports SoulSilver builder (" + TAG + ")", True)
    write_status("Screen: " + args.layout + ", world detail " + str(args.wide_lines) +
                 (", smooth scaling" if args.smooth else ", sharp pixels"), True)
    write_status("ROM stays on this device. It is not uploaded to GitHub or any build server.", True)

    root = checkout(Path.home() / ".local" / "share" / "dsports" / ("ssrecomp-" + TAG))
    write_status("Build workspace: " + str(root), True)
    setup = load_setup(root)
    patch_frontend_defaults(root, args)
    setup.android_steps = create_embedded_android_steps(setup, args, out)

    write_status("Checking the local ROM and generating native game code.", True)
    try:
        setup.run_steps(rom, args.jobs, log, progress, android=True)
        verify_apk(out)
        write_status("BUILD_SUCCEEDED", True)
        write_status("PORT_APK=" + out, True)
        write_status("Your port APK includes the locally supplied game. Keep it private.", True)
        return 0
    except Exception as exc:
        write_status("BUILD_FAILED", True)
        write_status(type(exc).__name__ + ": " + str(exc), True)
        traceback.print_exc()
        return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except SystemExit:
        raise
    except Exception as exc:  # pragma: no cover - last-resort reporting
        write_status("BUILD_FAILED", True)
        write_status("ERROR: " + str(exc), True)
        sys.exit(1)
