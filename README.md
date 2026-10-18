# DSports Android

DSports is an experimental on-device native recompilation builder for Pokémon SoulSilver, designed for ARM64 Android handhelds. **The separate Termux app is no longer required.** DSports creates its own private build environment inside its application storage.

## Build a port in DSports

1. Install and open DSports.
2. Tap **Select SoulSilver ROM** and choose your own US `.nds` dump.
3. Tap **Build playable Android APK** and allow the local build to finish.
4. Tap **Install ported game** when DSports reports success.

On the first build, DSports downloads a SHA-256-pinned ARM64 bootstrap environment from the official Termux package release archive, installs Python, Git, Clang/LLVM, CMake, Ninja and support tools in DSports's private storage, then runs the SSRecomp pipeline locally. It also downloads the Android NDK headers/sysroot and SDL2 source required by the recompiler. The toolchain can use several gigabytes of storage, substantial RAM, and a long uninterrupted build.

## Privacy

- The selected ROM is copied into DSports's private app storage for processing.
- DSports does not upload the ROM to GitHub or a build server.
- The generated `SoulSilver-ssrecomp.apk` is copied to `Download/DSports` for installation. It contains the game copy needed to play; keep it private and do not upload or redistribute it.
- The GitHub Actions workflow builds the DSports utility only. It does not receive a ROM and does not produce a game-containing APK.

## Important status

This is an experimental standalone build environment, not a guarantee that a working SoulSilver port will compile on every device. The workflow can validate that the DSports Android app compiles, but the full on-device native recompile has not yet been verified end-to-end on an AYN Thor. The build may still encounter Android toolchain, package-relocation, or upstream SSRecomp compatibility errors; check the in-app log if this happens.

The embedded runtime uses a renamed package prefix so the app can run Termux-built Android-native utilities without needing the Termux app. On Android, the package ID is `com.dsprth`; the visible app name remains DSports. This hotfix uses package ID `com.dsprth` so it can be installed alongside the earlier `com.dsprts` build without relying on the previous debug-signing key. After verifying the hotfix, you can remove the old copy.

## Development

- Kotlin UI and local orchestration: `app/src/main/java/com/dsports/library/MainActivity.kt`
- Private runtime bootstrap and package setup: `app/src/main/java/com/dsports/library/EmbeddedToolchain.kt`
- SSRecomp on-device builder: `app/src/main/assets/dsports_embedded_builder.py`
- JNI bootstrap: `app/src/main/cpp/native_port.cpp`

The underlying native pipeline uses the open-source [SSRecomp](https://github.com/ClickingMaster/ssrecomp) project at tag `v0.3.0`. This app's JNI bootstrap alone is not the SoulSilver runtime; the playable game APK is generated separately from the user's local ROM.
