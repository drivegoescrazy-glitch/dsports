# Native runtime status

DSports now builds and loads a small Android NDK/C++ shared library through JNI. This proves the project can package a native library into its APK and call it from Kotlin.

**This is not yet a playable SoulSilver port.** The C++ library only reports its bootstrap/API status. It does not run the game's ARM9/ARM7 code, render DS graphics, produce DS audio, parse game assets, or convert a ROM.

## Next implementation milestones

1. Review the current state and build requirements of the public HeartGold/SoulSilver code reconstruction project: https://github.com/pret/pokeheartgold
2. Keep any reference ROM and extracted proprietary assets on the user's device; never add them to source control or CI.
3. Define interfaces for the game core, dual-screen rendering, input/touch, audio, save data, filesystem, and frame timing.
4. Choose and validate a legally reviewed code source that can actually be compiled for the Android ARM64 target.
5. Integrate one system at a time and add tests before enabling any "Play" action.

## CI

GitHub Actions compiles the Android app and the C++ shared library. CI must not receive ROMs, extracted assets, save files, or user archives.
