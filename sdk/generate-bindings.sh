#!/usr/bin/env bash
#
# Builds the Anti Timpa core as a native library for a mobile host, and lays out
# the packaging skeleton a bank/PSP would wrap into an SDK.
#
# This script is scaffolding. It has not been run to completion in the
# repository's development environment (no Android NDK, no macOS), so the
# artifacts it produces are a starting point rather than a verified SDK.
# See README.md.
#
# Usage:
#   ./generate-bindings.sh android
#   ./generate-bindings.sh ios
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAURI_DIR="$REPO_ROOT/src-tauri"
OUT_DIR="$REPO_ROOT/sdk/out"
LIB_NAME="anti_timpa_lib"
PROFILE="${PROFILE:-release}"

# The C-ABI symbols the host links against. Kept here so a rename in
# src-tauri/src/sdk.rs that is not mirrored here fails the build loudly.
REQUIRED_SYMBOLS=(
  "antitimpa_version"
  "antitimpa_analyze_payload"
  "antitimpa_free_string"
)

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: '$1' not found. $2" >&2
    exit 1
  fi
}

cargo_flags() {
  if [ "$PROFILE" = "release" ]; then echo "--release"; else echo ""; fi
}

verify_symbols() {
  local lib="$1"
  local nm_tool="${2:-nm}"
  require_cmd "$nm_tool" "Install binutils (or the NDK/llvm toolchain) to verify exports."
  for sym in "${REQUIRED_SYMBOLS[@]}"; do
    if ! "$nm_tool" -g "$lib" 2>/dev/null | grep -q "$sym"; then
      echo "error: required symbol '$sym' missing from $lib" >&2
      echo "       the host would fail to link. Check src-tauri/src/sdk.rs." >&2
      exit 1
    fi
  done
  echo "ok: all C-ABI symbols present in $(basename "$lib")"
}

build_android() {
  require_cmd cargo "Install Rust: https://rustup.rs"
  if [ -z "${ANDROID_NDK_HOME:-}" ] && [ -z "${NDK_HOME:-}" ]; then
    echo "error: set ANDROID_NDK_HOME (or NDK_HOME) to your NDK install." >&2
    exit 1
  fi
  echo "note: this requires 'rustup target add aarch64-linux-android armv7-linux-androideabi'"

  mkdir -p "$OUT_DIR/android/jni/arm64-v8a" "$OUT_DIR/android/jni/armeabi-v7a"
  for target in aarch64-linux-android armv7-linux-androideabi; do
    ( cd "$TAURI_DIR" && cargo build $(cargo_flags) --target "$target" --lib )
  done

  cp "$TAURI_DIR/target/aarch64-linux-android/$PROFILE/lib$LIB_NAME.so" \
     "$OUT_DIR/android/jni/arm64-v8a/"
  cp "$TAURI_DIR/target/armv7-linux-androideabi/$PROFILE/lib$LIB_NAME.so" \
     "$OUT_DIR/android/jni/armeabi-v7a/"

  verify_symbols "$OUT_DIR/android/jni/arm64-v8a/lib$LIB_NAME.so" \
                 "${NDK_HOME:-$ANDROID_NDK_HOME}/toolchains/llvm/prebuilt/"*/bin/llvm-nm

  cat <<'EOF'
Scaffold laid out at sdk/out/android/. To finish an .aar:
  1. Add an AndroidManifest.xml at the AAR root.
  2. Add a Kotlin wrapper declaring the three `external fun` symbols.
  3. `cd sdk/out/android && zip -r ../anti-timpa-sdk.aar .`
This step is NOT automated because the wrapper API is the partner's decision.
EOF
}

build_ios() {
  require_cmd cargo "Install Rust: https://rustup.rs"
  if [ "$(uname -s)" != "Darwin" ]; then
    echo "error: iOS builds require macOS + Xcode." >&2
    exit 1
  fi
  require_cmd xcodebuild "Install Xcode from the App Store."

  mkdir -p "$OUT_DIR/ios"
  ( cd "$TAURI_DIR" && cargo build $(cargo_flags) --target aarch64-apple-ios --lib )

  local static_lib="$TAURI_DIR/target/aarch64-apple-ios/$PROFILE/lib$LIB_NAME.a"
  verify_symbols "$static_lib" "nm"

  cat <<EOF
Static library: $static_lib
Scaffold laid out at sdk/out/ios/. To finish a .framework:
  1. Create a module map exposing the three C functions.
  2. Add a thin Swift wrapper around the CMSampleBuffer -> RGB conversion
     (see ios/CameraBridge.swift for the camera side).
  3. xcodebuild -create-xcframework ...
EOF
}

case "${1:-}" in
  android) build_android ;;
  ios) build_ios ;;
  *)
    echo "usage: $0 {android|ios}" >&2
    exit 2
    ;;
esac
