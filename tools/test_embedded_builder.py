#!/usr/bin/env python3
"""Executable tests for the parts of the on-device builder that need no device.

These cover the logic that used to fail silently late in a multi-gigabyte build:
the NDK toolchain patch, the display defaults that DSports has to inject, and the
final APK check that decides whether the port is installable.
"""
from __future__ import annotations

import importlib.util
import struct
import sys
import tempfile
import types
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILDER = ROOT / "app/src/main/assets/dsports_embedded_builder.py"


def load_builder():
    spec = importlib.util.spec_from_file_location("dsports_embedded_builder_test", str(BUILDER))
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def fail(message: str):
    print("FAIL:", message, file=sys.stderr)
    raise SystemExit(1)


LEGACY_TOOLCHAIN = '''\
set(ANDROID_C_COMPILER
  "${ANDROID_TOOLCHAIN_ROOT}/bin/clang${ANDROID_TOOLCHAIN_SUFFIX}")
set(ANDROID_CXX_COMPILER
  "${ANDROID_TOOLCHAIN_ROOT}/bin/clang++${ANDROID_TOOLCHAIN_SUFFIX}")
set(ANDROID_ASM_COMPILER
  "${ANDROID_TOOLCHAIN_ROOT}/bin/clang${ANDROID_TOOLCHAIN_SUFFIX}")
set(ANDROID_AR
  "${ANDROID_TOOLCHAIN_ROOT}/bin/llvm-ar${ANDROID_TOOLCHAIN_SUFFIX}")
set(ANDROID_RANLIB
  "${ANDROID_TOOLCHAIN_ROOT}/bin/llvm-ranlib${ANDROID_TOOLCHAIN_SUFFIX}")
set(ANDROID_STRIP
  "${ANDROID_TOOLCHAIN_ROOT}/bin/llvm-strip${ANDROID_TOOLCHAIN_SUFFIX}")
if(ANDROID_STL STREQUAL c++_static)
  list(APPEND ANDROID_LINKER_FLAGS "-static-libstdc++")
endif()
string(REPLACE ";" " " ANDROID_LINKER_FLAGS "${ANDROID_LINKER_FLAGS}")
set(CMAKE_SYSROOT "${ANDROID_TOOLCHAIN_ROOT}/sysroot")
'''

FRONTEND = '''\
struct Settings
{
    std::string layout = "merged";
    int focusMode = 0;
    bool fullscreen = false;
    bool smooth = false;
    bool wide = true;   // widescreen 3D overworld
    int wideLines = 720; // height the widescreen 3D is drawn at, at most
    bool byLabel = true;
};
'''

TOOL_NAMES = ("clang", "clang++", "llvm-ar", "llvm-ranlib", "llvm-strip")


def test_toolchain_patch(builder):
    with tempfile.TemporaryDirectory() as temp:
        ndk = Path(temp) / "ndk"
        base = ndk / "build" / "cmake"
        base.mkdir(parents=True)
        legacy = base / "android-legacy.toolchain.cmake"
        legacy.write_text(LEGACY_TOOLCHAIN, encoding="utf-8")

        prefix = Path(temp) / "prefix"
        (prefix / "bin").mkdir(parents=True)
        for name in TOOL_NAMES:
            (prefix / "bin" / name).write_text("", encoding="utf-8")

        patched = builder.patch_ndk_toolchain(ndk, lambda _line: None, prefix=str(prefix))
        if not patched:
            fail("toolchain patch reported no changed files")

        text = legacy.read_text(encoding="utf-8")
        for variable, executable in (
            ("ANDROID_C_COMPILER", "clang"),
            ("ANDROID_CXX_COMPILER", "clang++"),
            ("ANDROID_ASM_COMPILER", "clang"),
            ("ANDROID_AR", "llvm-ar"),
            ("ANDROID_RANLIB", "llvm-ranlib"),
            ("ANDROID_STRIP", "llvm-strip"),
        ):
            expected = 'set(' + variable + ' "$ENV{PREFIX}/bin/' + executable + '")'
            if expected not in text:
                fail("toolchain patch did not persist: " + expected)
        if "CMAKE_SYSROOT" not in text or "${ANDROID_TOOLCHAIN_ROOT}/sysroot" not in text:
            fail("toolchain patch removed the NDK target sysroot")
        if "-static-libstdc++" not in text:
            fail("toolchain patch did not restore the NDK's required static libc++ link flag")
        if "${ANDROID_TOOLCHAIN_ROOT}/bin/clang" in text:
            fail("toolchain patch left the x86_64 NDK compiler selected")

        # An older DSports release removed this flag. Repair such cached NDKs too.
        modern = base / "android.toolchain.cmake"
        modern.write_text('include("${CMAKE_CURRENT_LIST_DIR}/android-legacy.toolchain.cmake")\n', encoding="utf-8")
        legacy.write_text(legacy.read_text(encoding="utf-8").replace(
            '  list(APPEND ANDROID_LINKER_FLAGS "-static-libstdc++")', ""
        ), encoding="utf-8")
        restored = builder.patch_ndk_toolchain(ndk, lambda _line: None, prefix=str(prefix))
        repaired_text = legacy.read_text(encoding="utf-8")
        if not restored or "-static-libstdc++" not in repaired_text:
            fail("toolchain patch did not repair a previously stripped C++ runtime link flag")

        # A second pass must be a no-op, not a rewrite of already patched text.
        again = builder.patch_ndk_toolchain(ndk, lambda _line: None, prefix=str(prefix))
        if again:
            fail("toolchain patch is not idempotent: " + repr(again))

        # A missing embedded tool must fail loudly instead of half-configuring.
        (prefix / "bin" / "llvm-strip").unlink()
        try:
            builder.patch_ndk_toolchain(ndk, lambda _line: None, prefix=str(prefix))
        except RuntimeError:
            pass
        else:
            fail("toolchain patch accepted an incomplete embedded toolchain")


def test_frontend_defaults(builder):
    options = types.SimpleNamespace(
        layout="side", wide_lines=1080, smooth=True, widescreen=False, windowed=False
    )
    with tempfile.TemporaryDirectory() as temp:
        root = Path(temp)
        (root / "frontend").mkdir()
        source = root / "frontend" / "main.cpp"
        source.write_text(FRONTEND, encoding="utf-8")

        builder.patch_frontend_defaults(root, options, lambda _line: None)
        text = source.read_text(encoding="utf-8")
        for expected in (
            'std::string layout = "side";',
            "int wideLines = 1080;",
            "bool smooth = true;",
            "bool wide = false;",
            "bool fullscreen = true;",
        ):
            if expected not in text:
                fail("display default was not applied: " + expected)

        # No change means no rewrite, so Ninja does not rebuild the frontend.
        if builder.patch_frontend_defaults(root, options, lambda _line: None):
            fail("display defaults rewrote unchanged source")

        # An unrecognised upstream layout must fail the build, never be ignored.
        source.write_text("struct Settings { int unrelated = 0; };\n", encoding="utf-8")
        try:
            builder.patch_frontend_defaults(root, options, lambda _line: None)
        except RuntimeError:
            pass
        else:
            fail("display defaults silently accepted an unknown frontend source layout")


def make_apk(path: Path, *, include_libs=True, signed=True):
    entries = {
        "AndroidManifest.xml": b"\x03\x00\x08\x00manifest",
        "classes.dex": b"dex\n035\x00" + b"\x00" * 64,
        "resources.arsc": b"\x02\x00\x0c\x00" + b"\x00" * 64,
        "assets/rom.nds": bytes(range(256)) * 512,
    }
    if include_libs:
        entries["lib/arm64-v8a/libmain.so"] = b"\x7fELF" + b"\x00" * 128
        entries["lib/arm64-v8a/libSDL2.so"] = b"\x7fELF" + b"\x00" * 128
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as apk:
        for name, data in entries.items():
            apk.writestr(name, data)

    if signed:
        data = path.read_bytes()
        eocd = data.rfind(b"PK\x05\x06")
        cd_offset = struct.unpack_from("<I", data, eocd + 16)[0]
        block = b"\x00" * 24 + b"APK Sig Block 42"
        patched = bytearray(data[:cd_offset] + block + data[cd_offset:])
        new_eocd = eocd + len(block)
        struct.pack_into("<I", patched, new_eocd + 16, cd_offset + len(block))
        path.write_bytes(bytes(patched))


def test_apk_verification(builder):
    with tempfile.TemporaryDirectory() as temp:
        good = Path(temp) / "good.apk"
        make_apk(good)
        info = builder.verify_apk(good, min_bytes=1024)
        if info["size"] != good.stat().st_size:
            fail("APK verification reported the wrong size")

        unsigned = Path(temp) / "unsigned.apk"
        make_apk(unsigned, signed=False)
        try:
            builder.verify_apk(unsigned, min_bytes=1024)
        except RuntimeError as exc:
            if "unsigned" not in str(exc):
                fail("unsigned APK failed for the wrong reason: " + str(exc))
        else:
            fail("APK verification accepted an unsigned APK")

        no_libs = Path(temp) / "no-libs.apk"
        make_apk(no_libs, include_libs=False)
        try:
            builder.verify_apk(no_libs, min_bytes=1024)
        except RuntimeError as exc:
            if "libmain.so" not in str(exc):
                fail("incomplete APK failed for the wrong reason: " + str(exc))
        else:
            fail("APK verification accepted an APK without the game library")

        tiny = Path(temp) / "tiny.apk"
        tiny.write_bytes(b"PK\x03\x04")
        try:
            builder.verify_apk(tiny)
        except RuntimeError:
            pass
        else:
            fail("APK verification accepted a truncated file")


def test_arguments(builder):
    args = builder.parse_args(
        ["--rom", "a.nds", "--status", "s.txt", "--output", "o.apk",
         "--layout", "side", "--resolution", "1080", "--smooth", "--no-widescreen"]
    )
    if args.layout != "side" or args.wide_lines != 1080:
        fail("display arguments were not parsed")
    if not args.smooth or args.widescreen:
        fail("display flags were not parsed")
    if args.jobs < 1:
        fail("jobs must be clamped to at least one")

    args = builder.parse_args(["--rom", "a", "--status", "s", "--output", "o", "--jobs", "99"])
    if args.jobs > 8:
        fail("jobs must be clamped to the handheld ceiling")
    if args.layout != builder.DEFAULT_LAYOUT:
        fail("the default layout changed unexpectedly")
    if args.wide_lines != builder.RESOLUTIONS[builder.DEFAULT_RESOLUTION]:
        fail("the default world detail changed unexpectedly")

    try:
        builder.parse_args(["--rom", "a", "--status", "s", "--output", "o", "--layout", "wat"])
    except SystemExit:
        pass
    else:
        fail("an unsupported layout was accepted")


def main():
    builder = load_builder()
    test_toolchain_patch(builder)
    test_frontend_defaults(builder)
    test_apk_verification(builder)
    test_arguments(builder)
    print("OK: builder toolchain patch, display defaults, APK verification and arguments.")


if __name__ == "__main__":
    main()
