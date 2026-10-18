# SoulSilver native-port feasibility and implementation plan

## Reference architecture

The linked Minish Cap Android project is not a generic ROM converter. It builds a native C/C++ port from an existing decompilation, then supplies Android-specific packaging and a platform layer for rendering, audio, input and assets. Its ROM is used locally to supply/extract game data; the repository does not distribute that game data.

SoulSilver is a Nintendo DS title, so a native port must account for both DS CPUs, 2D/3D graphics, touch input, audio, timing, saves, filesystem and networking-related game subsystems. Extracting the ROM's files and wrapping them in an APK would not be a native port.

## Safe local diagnostics already in the app

The current app can read the selected file locally, inspect its DS header, display the game code and compute a SHA-1 checksum. This does not prove a valid dump, and it does not convert or execute the game. The app does not send the file to a server or save a copy.

## Milestones before native gameplay is possible

1. **Source and licensing review:** evaluate an appropriate HeartGold/SoulSilver decompilation/disassembly and its terms. Do not assume code or assets are redistributable just because a repository is public.
2. **Reproducible source build:** prove the chosen codebase can build reproducibly from a clean checkout using a legally obtained reference copy locally. Keep the reference ROM and extracted proprietary data out of Git, CI logs, artifacts and releases.
3. **Platform boundary:** inventory every DS-specific API used by the game and define Android replacements. This is substantial work, not a one-click conversion.
4. **Rendering proof of concept:** bring up a small, isolated scene on Android, first validating display orientation, dual-screen layout, input and frame pacing without including game assets.
5. **Audio, saves and system services:** implement and test the remaining platform services with local-only game data.
6. **Compatibility and regression:** compare behaviour against known checkpoints; keep game-specific data out of CI and public build artifacts.
7. **Release only when truthful:** the APK must not advertise native SoulSilver gameplay until the native runtime actually runs the game. Keep the current diagnostic tool clearly labelled as diagnostics.

## Non-negotiable file handling

- Never commit or upload the user's ROM/archive or extracted game assets.
- Do not pass ROM bytes into GitHub Actions, issue comments, logs, build artifacts or release assets.
- Do not modify the source ROM.
- All current diagnostics are local and read-only.
