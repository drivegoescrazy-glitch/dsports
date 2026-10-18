# SoulSilver native runtime and Android build

## The practical native-port route

DSports now has a small C++/JNI bootstrap, but that bootstrap is not itself a playable port. The most direct existing native recompilation route found for SoulSilver is [ClickingMaster/ssrecomp](https://github.com/ClickingMaster/ssrecomp). It statically recompiles SoulSilver's ARM9 code to C++ and uses a patched melonDS core for Nintendo DS hardware services, including graphics, sound, saves and ARM7. The project documents an experimental Android build.

The Android APK is built on a **PC**, using the user's own SoulSilver dump. The upstream project states that the first build may take 20–40 minutes and that the resulting APK contains the game. This is not a one-tap conversion inside the current DSports Android app.

## Build on Windows

From the DSports checkout, open PowerShell and run:

```powershell
.\tools\build_soulsilver_android.ps1 -RomPath "D:\Games\SoulSilver.nds"
```

Optional parameters:

```powershell
.\tools\build_soulsilver_android.ps1 -RomPath "D:\Games\SoulSilver.nds" -OutputPath "D:\PrivateBuilds\SoulSilver.apk" -Jobs 4
```

The helper pins the SSRecomp source to tag `v0.3.0` and keeps source, generated code, build files and the source ROM path outside the DSports checkout under the user's local application data directory. It requires Git, Python 3, CMake and Ninja on the PC; SSRecomp downloads its Android NDK and SDL2 build dependencies. Windows/Android support is experimental, so failures must be debugged from the build output.

After building, transfer the APK to your own Android device and install it. The APK contains your copy of the game; do not upload it to GitHub, attach it to releases, or share it.

## Boundaries

- Never commit the ROM, extracted assets, generated game code, save data, or the game-containing APK.
- Never send ROM bytes to GitHub Actions, logs, issues, or release artifacts.
- The DSports GitHub Actions job only compiles the DSports app and its small JNI bootstrap; it does not receive game data.
- The SSRecomp source has its own license and project terms. Review them before redistributing or incorporating its source into DSports.

## DSports native bootstrap

The C++ library under `app/src/main/cpp/` currently only confirms that JNI and NDK packaging work. It does not execute SoulSilver. Do not advertise the DSports APK itself as a playable native SoulSilver port until the actual runtime is integrated and tested.
