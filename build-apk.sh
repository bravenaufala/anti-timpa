#!/usr/bin/env bash
#
# Build, sign, and optionally install the Android release APK.
#
# Why this exists: the release path has two non-obvious requirements that are
# easy to get wrong by hand.
#
#   1. The release APK must be signed before Android will install it. An
#      "unsigned" APK fails with INSTALL_PARSE_FAILED_NO_CERTIFICATES.
#   2. The JNI symbols must survive LTO and stripping. This is handled by
#      build.rs, but the script verifies it, because a release-only
#      UnsatisfiedLinkError is otherwise only discovered on a device.
#
# Usage:
#   ./build-apk.sh              # build + sign
#   ./build-apk.sh --install    # build + sign + install to connected device

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# --- Configuration -----------------------------------------------------------
# Override any of these via the environment.
ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | tail -1)}"

KEYSTORE="${KEYSTORE:-$SCRIPT_DIR/keystore/antitimpa.keystore}"
KEY_ALIAS="${KEY_ALIAS:-antitimpa}"
STORE_PASS="${STORE_PASS:-antitimpa2026}"
KEY_PASS="${KEY_PASS:-antitimpa2026}"

OUT_DIR="$SCRIPT_DIR/dist-apk"
OUT_APK="$OUT_DIR/antitimpa-release.apk"

UNSIGNED_APK="$SCRIPT_DIR/src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk"

# gen/ is not tracked in git. It is produced by `npx tauri android init`, and
# the Kotlin camera bridge plus CameraX dependencies are installed into it by
# hand (see android/README.md). Without that, this build succeeds but the
# camera never delivers a frame.
[ -d "$SCRIPT_DIR/src-tauri/gen/android" ] || fail "src-tauri/gen/android tidak ada.
  Jalankan 'npx tauri android init' lalu pasang jembatan kamera sesuai
  android/README.md (dependensi CameraX, izin CAMERA, CameraBridge.kt,
  CameraFrameAnalyzer.kt, dan pemanggilan dari MainActivity)."

# Build tools version is discovered rather than hardcoded, so a different SDK
# install does not silently pick the wrong apksigner.
BUILD_TOOLS="$(ls -d "$ANDROID_HOME"/build-tools/* 2>/dev/null | sort -V | tail -1)"

info()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m!!\033[0m %s\n' "$*" >&2; }
fail()  { printf '\033[1;31mxx\033[0m %s\n' "$*" >&2; exit 1; }

# --- Preflight ---------------------------------------------------------------
[ -d "$ANDROID_HOME" ] || fail "Android SDK tidak ditemukan di $ANDROID_HOME (set ANDROID_HOME)"
[ -n "$BUILD_TOOLS" ]  || fail "build-tools tidak ditemukan di $ANDROID_HOME/build-tools"
[ -n "$ANDROID_NDK_HOME" ] || fail "NDK tidak ditemukan di $ANDROID_HOME/ndk (set ANDROID_NDK_HOME)"
[ -f "$KEYSTORE" ] || fail "Keystore tidak ada di $KEYSTORE. Buat dulu:
  keytool -genkeypair -v -keystore '$KEYSTORE' -alias '$KEY_ALIAS' \\
    -keyalg RSA -keysize 2048 -validity 10000"

info "SDK       : $ANDROID_HOME"
info "NDK       : $ANDROID_NDK_HOME"
info "apksigner : $(basename "$BUILD_TOOLS")"

# --- Build -------------------------------------------------------------------
info "Membangun frontend (Vite)..."
npm run build >/dev/null

info "Membangun APK release (arm64-v8a)..."
# TAURI_* variables are read by the CLI; exported so the Gradle invocation sees
# the same NDK the preflight resolved.
#
# `--features geolocation` is required here: the Tauri geolocation plugin is an
# optional dependency, and mobile builds must opt in. Without it the plugin is
# not registered and Layer 3 reports "location unavailable" on every scan even
# though the OS location service and the permission prompt are both available.
export ANDROID_HOME ANDROID_NDK_HOME
export TAURI_ANDROID_FEATURES="geolocation"
npx tauri android build --apk --target aarch64 --features geolocation

[ -f "$UNSIGNED_APK" ] || fail "APK tidak dihasilkan di $UNSIGNED_APK"

# --- Verify JNI symbols survived --------------------------------------------
# This is the check that matters most. Release builds enable LTO and
# `strip = true`, which removes the JNI entry points unless build.rs passes
# -Wl,--undefined for each one. The app then runs until the first camera call
# and dies with UnsatisfiedLinkError — a failure that never appears in debug.
info "Memverifikasi simbol JNI masih ada di .so release..."

VERIFY_DIR="$(mktemp -d)"
trap 'rm -rf "$VERIFY_DIR"' EXIT

unzip -o -q "$UNSIGNED_APK" "lib/*" -d "$VERIFY_DIR"

SO_FILE="$(find "$VERIFY_DIR/lib" -name 'libanti_timpa_lib.so' | head -1)"
[ -n "$SO_FILE" ] || fail "libanti_timpa_lib.so tidak ada di dalam APK"

MISSING=0
for SYMBOL in \
  Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1push_1frame \
  Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1set_1stream_1active \
  Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1frames_1received
do
  if ! nm -D --defined-only "$SO_FILE" 2>/dev/null | grep -q "$SYMBOL"; then
    warn "simbol hilang: $SYMBOL"
    MISSING=1
  fi
done

if [ "$MISSING" -ne 0 ]; then
  fail "Simbol JNI hilang dari build release. Periksa build.rs —
       pastikan setiap simbol ada di daftar --undefined.
       Tanpa ini, app akan force close saat kamera dipanggil."
fi
info "Ketiga simbol JNI ada."

# --- Sign --------------------------------------------------------------------
info "Menandatangani APK..."
mkdir -p "$OUT_DIR"
"$BUILD_TOOLS/apksigner" sign \
  --ks "$KEYSTORE" \
  --ks-key-alias "$KEY_ALIAS" \
  --ks-pass "pass:$STORE_PASS" \
  --key-pass "pass:$KEY_PASS" \
  --out "$OUT_APK" \
  "$UNSIGNED_APK"

"$BUILD_TOOLS/apksigner" verify "$OUT_APK" >/dev/null \
  || fail "Verifikasi signature gagal"

SIZE="$(du -h "$OUT_APK" | cut -f1)"
info "APK siap: $OUT_APK ($SIZE)"

# --- Optionally install ------------------------------------------------------
if [ "${1:-}" = "--install" ]; then
  # Prefer the platform-tools bundled with the configured SDK over whatever is
  # on PATH, so the version matches the SDK we just used.
  ADB="$ANDROID_HOME/platform-tools/adb"
  [ -x "$ADB" ] || ADB="adb"

  "$ADB" devices | grep -qw device || fail "Tidak ada perangkat terhubung. Jalankan 'adb devices'."

  info "Memasang ke perangkat..."
  # Uninstall first: the release APK is signed with a different key than any
  # debug build, so Android refuses to replace one with the other.
  "$ADB" uninstall org.antitimpa.antitimpa >/dev/null 2>&1 || true
  "$ADB" install "$OUT_APK"

  # Grant the camera permission up front so the log is not dominated by the
  # permission dialog during verification. A real user would see the prompt.
  "$ADB" shell pm grant org.antitimpa.antitimpa android.permission.CAMERA 2>/dev/null || true

  # Location is deliberately NOT pre-granted. The whole point of Layer 3 asking
  # at scan time is that the user makes an informed choice, and auto-granting
  # during a test install would hide whether the permission prompt actually works.
  info "Izin lokasi sengaja tidak di-grant otomatis — uji prompt manualnya."

  info "Terpasang. Untuk melihat log kamera:"
  echo "    $ADB logcat -s ANTITIMPA RustStdoutStderr | grep -i camera"
fi
