# DSports Android

DSports now has an **experimental on-device SoulSilver APK builder**. It selects a user's local Nintendo DS ROM, invokes the SSRecomp native recompiler through Termux, builds the Android runtime locally, and packages a separate playable APK. This is intended for ARM64 Android handhelds such as the AYN Thor.

## Build a port inside DSports

1. Install and open Termux on the handheld.
2. In Termux, run `termux-setup-storage`.
3. Enable external command requests in Termux:
   ```sh
   mkdir -p ~/.termux
   printf '\nallow-external-apps = true\n' >> ~/.termux/termux.properties
   ```
   Close and reopen Termux after changing the setting.
4. In Android Settings, allow DSports **Run commands in Termux environment** permission, and grant Termux storage access.
5. Open DSports, select your own **US Pokémon SoulSilver .nds** dump, press **Build playable Android APK**, and leave the Termux build session running. Then tap **Install ported game** when DSports reports success.

The first build installs local compiler packages, downloads the Android NDK and SDL2 source, recompiles game code from the selected ROM, and compiles the native runtime. The process is compute- and storage-intensive and may take a long time. The builder conservatively uses two parallel jobs.

## ROM privacy

- The ROM is read and processed locally on the handheld. DSports does not upload it to GitHub or a build server.
- DSports temporarily stages a copy under `Download/DSports/soulsilver.nds` so Termux can read it. After a successful build, DSports deletes that staged copy.
- The generated `Download/DSports/SoulSilver-ssrecomp.apk` contains the game copy needed to play. Keep that APK private and do not upload or redistribute it.
- Generated C++ and compiler/build files remain in Termux's private application storage. No ROM, generated game code, save file, or game-containing APK is committed to this repository or CI artifacts.

## Implementation and limitations

The app uses the open-source [SSRecomp project](https://github.com/ClickingMaster/ssrecomp) at release tag `v0.3.0`. The bundled `dsports_termux_builder.py` adapts its Android build steps to use Termux's on-device ARM64 compiler with Android NDK headers/libraries. SSRecomp recompiles game code to C++ and combines it with a modified melonDS runtime. That is an actual native-recompilation-based game build, not just copying a ROM into a generic emulator wrapper.

This on-device toolchain is experimental. Successful compilation depends on the Termux version, available storage/RAM, and whether the Android NDK toolchain works with Termux's compiler. The GitHub Actions workflow only verifies the DSports Android app itself builds; it does not have access to the user's ROM and does not run the on-device game build.

## Development

The app is written in Kotlin with a small JNI/C++ bootstrap. The local port pipeline lives in `app/src/main/assets/dsports_termux_builder.py`; app UI and Termux orchestration live in `app/src/main/java/com/dsports/library/MainActivity.kt`.

GitHub Actions builds a debug APK on pushes to `main`. A version tag such as `v0.3.0` publishes an APK release. The DSports debug APK is a local porting tool; it is not itself a playable SoulSilver port. The separate SoulSilver APK is generated only on the user's device from the user's own ROM.
