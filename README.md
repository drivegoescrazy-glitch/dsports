# DSports Android

DSports is an on-device native recompilation builder for Pokémon SoulSilver, designed for ARM64 Android handhelds. **The separate Termux app is not required.** DSports creates its own private build environment inside its application storage.

## Launch and build a port

1. Install and open DSports. The **launcher** lists every supported game. SoulSilver is the only supported title today; further games appear there as their pipelines are verified.
2. Tap **Open Pokémon SoulSilver**.
3. Tap **Select Pokémon SoulSilver ROM** and choose your own US `.nds` dump.
4. Optionally set the port's display defaults: screen layout (merged, dual side-by-side, dual stacked, hybrid), world detail (native 192 to 1440 lines), smooth scaling and the widescreen world.
5. Tap **Build playable Android APK** and allow the local build to finish.
6. Tap **Install ported game** when DSports reports success.

The port's own pause menu (the on-screen **MENU** button, `Esc`, or controller Guide / Start+Select) exposes the same display settings while playing, so nothing chosen here is permanent.

On the first build DSports downloads a SHA-256-pinned ARM64 bootstrap environment from the official Termux package release archive, installs Python, Git, Clang/LLVM, CMake, Ninja and support tools in DSports's private storage, then runs the SSRecomp v0.4.2 pipeline locally. It also downloads the Android NDK headers/sysroot and SDL2 source required by the recompiler. The toolchain can use several gigabytes of storage, substantial RAM, and a long uninterrupted build.

Before compiling, DSports:

- rewrites Termux `.deb` data paths and maintainer scripts for its own sandbox and keeps binary executables untouched;
- points the NDK toolchain at the device's own ARM64 compiler, assembler, archiver and strip tools, preserving `-static-libstdc++`, the NDK's supported Clang option for statically linking libc++;
- compiles and links a tiny C and C++ shared library first, so a broken sysroot or compiler fails in seconds instead of after a long compile;
- verifies the finished APK (archive integrity, `libmain.so`, `libSDL2.so`, the game asset and the APK Signature Scheme v2 block) before reporting success.

## Privacy

- The selected ROM is copied into DSports's private app storage for processing.
- DSports does not upload the ROM to GitHub or a build server.
- The generated `SoulSilver-ssrecomp.apk` is copied to `Download/DSports` for installation. It contains the game copy needed to play; keep it private and do not upload or redistribute it.
- The GitHub Actions workflow builds the DSports utility only. It does not receive a ROM and does not produce a game-containing APK.

## Important status

DSports builds the port entirely on the device, but the full on-device native recompile has not been verified end-to-end on every handheld. The workflow validates the DSports app, the builder's logic and its APK handling; it does not receive a ROM. If a build fails, the in-app log now ends with the specific stage that failed (toolchain preflight, NDK, SDL2, game compile or APK verification).

The embedded runtime uses a renamed package prefix so the app can run Termux-built Android-native utilities without needing the Termux app. On Android, the package ID is `com.dsprtm`; the visible app name remains DSports. This build is installable alongside earlier `com.dsprts`/`com.dsprtk`/`com.dsprtl` copies; remove those once the new one is verified.

## Development

- Launcher and supported-game catalogue: `app/src/main/java/com/dsports/library/LauncherActivity.kt`, `GameCatalog.kt`
- Kotlin UI and local orchestration: `app/src/main/java/com/dsports/library/MainActivity.kt`
- Private runtime bootstrap and package setup: `app/src/main/java/com/dsports/library/EmbeddedToolchain.kt`
- SSRecomp on-device builder: `app/src/main/assets/dsports_embedded_builder.py`
- Builder unit tests (no device needed): `tools/test_embedded_builder.py`
- Static regression checks: `tools/check_standalone_runtime.py`
- JNI bootstrap: `app/src/main/cpp/native_port.cpp`

Run the local checks with:

```bash
python3 tools/check_standalone_runtime.py
python3 tools/test_embedded_builder.py
python3 tools/test_dpkg_wrapper.py
```

The underlying native pipeline uses the open-source [SSRecomp](https://github.com/ClickingMaster/ssrecomp) project at tag `v0.4.2`. This app's JNI bootstrap alone is not the SoulSilver runtime; the playable game APK is generated separately from the user's local ROM.
