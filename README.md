# DSports Android

An Android game-library prototype for user-selected Nintendo DS game folders. It uses Android's Storage Access Framework to read directory entries and identify `.nds` files without modifying them.

## Current status
- Read-only folder scanning and game list UI.
- Pokémon SoulSilver detection by filename (not ROM verification).
- Dual-screen-friendly layout foundation.
- GitHub Actions workflow to build a debug APK and publish it as a workflow artifact.

**Important:** This prototype does not contain a Nintendo DS emulator or convert a DS ROM into a native Android game. SoulSilver gameplay requires a separately installed compatible DS emulator; launching into one is not yet implemented. Use only game files you are legally entitled to use.

## Build
Open this project in Android Studio or run `./gradlew assembleDebug`. GitHub Actions builds the debug APK on pushes to `main`.
