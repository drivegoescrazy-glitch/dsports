# DSports Android

DSports is an Android game-library and native-port research project. The app can scan a folder selected by the user for Nintendo DS .nds files and run a local, read-only header/hash inspection on a likely Pokémon SoulSilver file.

## Privacy guarantee for user game files

- The app reads a ROM only after the user explicitly selects its folder.
- The current inspection streams bytes locally to read the DS header and calculate a SHA-1 checksum.
- It does not upload, copy into the repository, or modify the ROM.
- No ROM, extracted game assets, save data, or user-provided game archive should ever be committed to this repository or attached to a release.

The SoulSilver archive shared in the development chat is reference material only. It is not part of the repository or build inputs.

## Native-port status

**Native-port status: bootstrap only; SoulSilver gameplay is not implemented.** DSports now packages a small C++ Android NDK library and calls it through JNI, which establishes the native build/bridge foundation. The app still only performs local ROM diagnostics; it does not convert a ROM or run the game. A real native port, like the Minish Cap project, needs reconstructed game logic plus replacement platform services for Nintendo DS-specific systems (ARM9/ARM7, graphics, touch, audio, saves, filesystem and timing). A simple ROM-to-APK packaging step would still be emulation, not a native port.

The Minish Cap example works because it builds a native C/C++ port from an existing decompilation and its own platform/renderer/audio layers; it does not magically translate a ROM into native Android source. The public pret/pokeheartgold project is a work-in-progress disassembly, not a ready-made Android native runtime. The project now includes a local Windows build helper that uses the existing SSRecomp native recompilation project to create a game-containing Android APK from the user's own ROM. From PowerShell on a PC, run `.\tools\build_soulsilver_android.ps1 -RomPath "D:\\Games\\SoulSilver.nds"`. The first build may take 20–40 minutes. The resulting APK includes game data and must remain private; the script keeps its source/build files outside this repository. This is an experimental external build route, not an on-device converter. See `docs/native-runtime-status.md` for requirements and limits.

## Current features

- Read-only folder scanning through Android's Storage Access Framework.
- Filename-based candidate detection for SoulSilver.
- On-device DS header, title, game-code and SHA-1 diagnostics.
- No network code for ROM handling.
- C++ native library compiled into the APK through the Android NDK/CMake and a Kotlin JNI bridge.
- GitHub Actions debug APK build and release-on-tag workflow.

## Build and release

Build locally with Android Studio or gradle assembleDebug in an environment with the Android SDK configured. GitHub Actions builds an APK on pushes to main. Pushing a version tag such as v0.1.0 builds and attaches the APK to a GitHub Release. This is currently a debug-signed prototype, not a production-ready native port.
