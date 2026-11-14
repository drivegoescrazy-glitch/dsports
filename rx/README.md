# NXS Launcher 1.0.10

A CardsNX-style Android launcher, rewritten in Rust. v1.0.2 replaces the Kotlin
view stack with a single native renderer: the whole launcher is one
`tiny-skia` scene rasterised into a pixmap and uploaded as one GL texture per
frame.

![Home screen](docs/v102-home.png)

## What ships

- **Exact CardsNX layout** — coverflow card row with a focused card rim, glass
  info panel (icon, title, meta, version chip, Play pill, overflow, layout
  circles, page dots) and the circular top bar.
- **Dual screen** — Settings → Dual screen moves the game preview to the bottom
  screen area with three widgets above it: play activity across the top, storage
  and online side by side in the middle.
- **Custom media** — pick a box art icon or a hero background per game from the
  Settings sheet (system photo picker, no storage permission). Media is stored
  per package and reloaded on the next launch.
- **Render on demand** — the render thread sleeps on a condvar and only redraws
  when something actually changed, waking on a 30 s timer for the clock. An idle
  launcher draws nothing, which is what keeps it cool on a handheld.

## Size and speed

| | v1.0.1 (Kotlin) | v1.0.2 (Rust) |
| --- | --- | --- |
| Native core | — | 954 KB arm64 / 685 KB armv7 |
| APK (both ABIs) | — | 1.7 MB |
| Frames while idle | continuous invalidate | zero |

The Kotlin shell is ~150 lines: it owns the `SurfaceView`, forwards
surface/input/lifecycle events to JNI and answers three callbacks (launch a
package, open home settings, open the photo picker). Everything else — layout,
text shaping, blending, input hit testing, state — is Rust.

## Layout

```
rust/src/
  theme.rs    palette and the 1920x1080 design grid
  text.rs     ab_glyph outlines -> tiny-skia paths, measuring, ellipsis
  scene.rs    command list and painter (rounded rects, images, gradients)
  ui.rs       glass surfaces, glyphs, pills, dots
  model.rs    apps, pages, sort, overlays, touches
  home.rs     hero wash, top bar, coverflow, info panel
  dual.rs     activity / storage / online widgets + game preview
  android.rs  EGL surface, JNI app enumeration, icons, preferences
  lib.rs      scene assembly and the settings sheet
app/src/main/java/com/nxslauncher/NxsActivity.kt
```

## Building

```sh
export ANDROID_SDK_ROOT=$HOME/Android/Sdk   # must contain an NDK
./build-rust.sh                              # cargo build for both ABIs
gradle :app:assembleRelease
```

`build-rust.sh` cross-compiles `libnxs.so` for `arm64-v8a` and `armeabi-v7a`
into `app/src/main/jniLibs`; Gradle packages them uncompressed so the loader
maps them straight out of the APK.

Requires Rust stable with the `aarch64-linux-android` and
`armv7-linux-androideabi` targets, an NDK (r26+) and JDK 17.

## Host preview

The renderer is platform independent, so the exact same scene can be written to
a PNG instead of a window:

```sh
cd rust
cargo run --features preview --bin nxs-preview -- docs/home.png
cargo run --features preview --bin nxs-preview -- --dual docs/dual.png
cargo run --features preview --bin nxs-preview -- --settings docs/settings.png
```
