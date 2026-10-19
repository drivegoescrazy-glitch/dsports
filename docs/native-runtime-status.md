# SoulSilver native runtime and Android build

## Current route

DSports builds the playable port **on the device** with [ClickingMaster/ssrecomp](https://github.com/ClickingMaster/ssrecomp), pinned to tag `v0.4.2`. SSRecomp statically recompiles SoulSilver's ARM9 code to C++ and uses a patched melonDS core for Nintendo DS hardware services, including graphics, sound, saves and ARM7.

The APK is produced from the user's own dump. Nothing of the game is downloaded, and no ROM bytes leave the device.

## What DSports adds on top of upstream

| Concern | DSports behaviour |
| --- | --- |
| Host compilers | The downloaded NDK is a Linux/x86_64 bundle whose host binaries cannot run on Android. DSports rewrites `ANDROID_C_COMPILER`, `ANDROID_CXX_COMPILER`, `ANDROID_ASM_COMPILER`, `ANDROID_AR`, `ANDROID_RANLIB` and `ANDROID_STRIP` to the embedded ARM64 tools and keeps the NDK's headers, crt objects and per-ABI libraries. |
| Link flags | The NDK's CMake toolchain adds `-static-libstdc++` to select its `libc++_static.a` runtime with Clang. DSports preserves this required flag; without it the APK would depend on an unbundled `libc++_shared.so`. |
| Package lookup | SDL2 is built outside the NDK, so package lookup is widened with `CMAKE_FIND_ROOT_PATH_MODE_PACKAGE=BOTH` instead of being restricted to the NDK. |
| Fail fast | Before configuring anything, DSports compiles and links a C and a C++ shared library for `aarch64-linux-android24` and checks that the result is an ARM64 ELF. A missing sysroot, crt object or C++ standard library now fails in seconds with the compiler's own message. |
| Display defaults | The chosen layout, world detail, smooth scaling, widescreen and fullscreen settings are patched into the frontend's `Settings` struct. Each pattern must match exactly once, so an upstream layout change fails the build instead of silently shipping the wrong defaults. |
| APK verification | The finished APK is opened as a ZIP, tested entry by entry, checked for `libmain.so`, `libSDL2.so`, `assets/rom.nds`, the manifest, `classes.dex` and the APK Signature Scheme v2 block, and rejected if any is missing. |

Display settings available both in DSports and in the port's pause menu:

- **Layout** — merged (one combined view), dual side-by-side, dual stacked, hybrid (large screen plus inset).
- **World detail** — the maximum number of lines the widescreen 3D world is drawn at: native 192, 540, 720, 1080 or 1440.
- **Smooth scaling** — filtered instead of nearest-pixel scaling.
- **Widescreen world** — the overworld 3D view fills the whole display width.

## Boundaries

- Never commit the ROM, extracted assets, generated game code, save data, or the game-containing APK.
- Never send ROM bytes to GitHub Actions, logs, issues, or release artifacts.
- The DSports GitHub Actions job only compiles the DSports app and exercises the builder's device-independent logic; it does not receive game data.
- The SSRecomp source has its own license and project terms. Review them before redistributing or incorporating its source into DSports.

## DSports native bootstrap

The C++ library under `app/src/main/cpp/` only confirms that JNI and NDK packaging work. It does not execute SoulSilver.
