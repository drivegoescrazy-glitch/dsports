# DSports Android

DSports is an Android game-library and native-port research project. The app can scan a folder selected by the user for Nintendo DS .nds files and run a local, read-only header/hash inspection on a likely Pokémon SoulSilver file.

## Privacy guarantee for user game files

- The app reads a ROM only after the user explicitly selects its folder.
- The current inspection streams bytes locally to read the DS header and calculate a SHA-1 checksum.
- It does not upload, copy into the repository, or modify the ROM.
- No ROM, extracted game assets, save data, or user-provided game archive should ever be committed to this repository or attached to a release.

The SoulSilver archive shared in the development chat is reference material only. It is not part of the repository or build inputs.

## Native-port status

**The native SoulSilver converter is not implemented yet.** The current app performs diagnostics only; it does not convert a ROM or run the game. A real native port, like the Minish Cap project, needs a reconstructed/decompiled game codebase plus a replacement platform layer for Nintendo DS-specific systems (ARM9/ARM7, graphics, touch, audio, saves, filesystem and timing). A simple ROM-to-APK packaging step would still be emulation, not a native port.

The Minish Cap example works because it builds a native C/C++ port from an existing decompilation and its own platform/renderer/audio layers; it does not magically translate a ROM into native Android source. The public pret/pokeheartgold project is a work-in-progress disassembly, not a ready-made Android native runtime. The next engineering milestone is a separate, legally reviewed native-runtime proof of concept and source/toolchain feasibility study—not falsely labelling the current scanner as a converter.

## Current features

- Read-only folder scanning through Android's Storage Access Framework.
- Filename-based candidate detection for SoulSilver.
- On-device DS header, title, game-code and SHA-1 diagnostics.
- No network code for ROM handling.
- GitHub Actions debug APK build and release-on-tag workflow.

## Build and release

Build locally with Android Studio or gradle assembleDebug in an environment with the Android SDK configured. GitHub Actions builds an APK on pushes to main. Pushing a version tag such as v0.1.0 builds and attaches the APK to a GitHub Release. This is currently a debug-signed prototype, not a production-ready native port.
