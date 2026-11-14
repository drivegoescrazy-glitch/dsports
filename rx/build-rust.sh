#!/usr/bin/env bash
# Builds libnxs.so for every shipped ABI and drops it where Gradle expects it.
# Requires: rustup targets, $ANDROID_SDK_ROOT with an NDK installed.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/tools/android-sdk}}"
NDK="${NDK_HOME:-$(ls -d "$SDK"/ndk/* 2>/dev/null | sort -V | tail -1)}"
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
EXPORT_PATH="$HOME/.cargo/bin:$PATH"
export PATH="$EXPORT_PATH"

if [[ ! -d "$TOOLCHAIN" ]]; then
  echo "NDK toolchain not found under $NDK" >&2
  exit 1
fi

build_abi() {
  local triple="$1" abi="$2" cc="$3"
  echo "== building $abi ($triple) =="
  local linker_var
  linker_var="CARGO_TARGET_$(echo "$triple" | tr '[:lower:]-' '[:upper:]_')_LINKER"
  local cc_var
  cc_var="CC_$(echo "$triple" | tr '-' '_')"
  env CARGO_TARGET_DIR="$ROOT/rust/target" \
      "$linker_var=$TOOLCHAIN/$cc" \
      "$cc_var=$TOOLCHAIN/$cc" \
      cargo build --manifest-path "$ROOT/rust/Cargo.toml" --release --target "$triple" --lib
  mkdir -p "$ROOT/app/src/main/jniLibs/$abi"
  cp "$ROOT/rust/target/$triple/release/libnxs.so" "$ROOT/app/src/main/jniLibs/$abi/libnxs.so"
  ls -lh "$ROOT/app/src/main/jniLibs/$abi/libnxs.so"
}

build_abi aarch64-linux-android arm64-v8a aarch64-linux-android26-clang
if [[ "${ARMV7:-1}" == "1" ]]; then
  build_abi armv7-linux-androideabi armeabi-v7a armv7a-linux-androideabi26-clang
fi

echo "native libraries staged in app/src/main/jniLibs"
