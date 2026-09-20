# Panduan Build — Anti Timpa QRIS Scanner

Aplikasi lintas-platform (desktop / Android / iOS) berbasis **React + Tauri +
Rust** yang menjalankan analisis QRIS **100% lokal per perangkat**.

> Struktur kode, ringkasan migrasi, dan daftar yang belum selesai ada di
> **[`README.md`](README.md)**.

| Platform | Framework | Catatan |
|---|---|---|
| **Desktop** (Win/macOS/Linux) | Tauri + Rust (`nokhwa`) | kamera via V4L2/AVFoundation/MSMF |
| **Android** (APK) | Tauri + Kotlin bridge | butuh SDK + NDK; dukung tanda tangan rilis |
| **iOS** | Tauri | belum ada bridge kamera AVFoundation |

---

## 0. Prasyarat umum

| Tool | Versi | Keterangan |
|---|---|---|
| Node.js | 18+ | untuk Vite + Tauri CLI |
| Rust | 1.77.2+ | `rust-version` di `src-tauri/Cargo.toml` |
| Tauri CLI | 2.x | terpasang lokal via `npm install` (`node_modules/.bin/tauri`) |

Linux desktop juga butuh `webkit2gtk` dan `libv4l` (untuk kamera). Untuk Ubuntu/Debian:

```bash
sudo apt install libwebkit2gtk-4.1-dev libv4l-dev build-essential
```

---

## 1. Install & jalankan

```bash
cd anti-timpa
npm install
```

### Desktop (dev)

```bash
npm run tauri:dev
```

### Desktop (build produksi)

```bash
npm run tauri:build
```

Hasil bundle ada di `src-tauri/target/release/bundle/`.

### UI saja (tanpa backend Rust)

```bash
npm run dev
```

Backend Rust tidak aktif di mode ini; UI menampilkan peringatan dan tombol
analisis tidak berfungsi. Berguna untuk mengubah tampilan tanpa menunggu
kompilasi Rust.

> **Catatan ruang disk.** Kompilasi Tauri butuh beberapa GB untuk codegen. Bila
> disk hampir penuh, `cargo clean` di `src-tauri/` lalu build ulang. Untuk
> sekadar memeriksa logika, pakai perintah validasi di bagian 3 yang jauh lebih
> ringan (`--no-default-features` melewatkan nokhwa/V4L2).

---

## 2. Build Android APK

### Cara cepat (disarankan)

```bash
./build-apk.sh              # build + sign
./build-apk.sh --install    # build + sign + install ke perangkat
```

Skrip ini melakukan empat hal yang mudah salah bila dikerjakan manual:

1. Membangun frontend Vite lebih dulu.
2. Menjalankan `npx tauri android build --apk --target aarch64`.
3. **Memverifikasi ketiga simbol JNI masih ada** di `libanti_timpa_lib.so`
   sebelum menandatangani. Build release memakai `lto = true` dan `strip = true`;
   tanpa `-Wl,--undefined` dari `build.rs`, entry point JNI hilang dan aplikasi
   mati dengan `UnsatisfiedLinkError` saat kamera pertama dipanggil — kegagalan
   yang **tidak pernah muncul di build debug**.
4. Menandatangani dengan `apksigner` lalu memverifikasi signature-nya.

APK jadi: `dist-apk/antitimpa-release.apk`.

Variabel yang bisa di-override:

```bash
ANDROID_HOME=... ANDROID_NDK_HOME=... KEYSTORE=... KEY_ALIAS=... \
STORE_PASS=... KEY_PASS=... ./build-apk.sh
```

### Prasyarat Android

```bash
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/<versi>"
```

Keystore rilis ada di `keystore/antitimpa.keystore`. Bila perlu dibuat ulang:

```bash
keytool -genkeypair -v -keystore keystore/antitimpa.keystore -alias antitimpa \
  -keyalg RSA -keysize 2048 -validity 10000
```

### Langkah manual (setara)

```bash
npm run build
npx tauri android init       # sekali saja, menghasilkan src-tauri/gen/android/
npx tauri android build --apk --target aarch64
```

`src-tauri/gen/` tidak masuk git (lihat `.gitignore`) dan dibuat ulang oleh
`android init`.

### Memasang jembatan kamera CameraX

Frame kamera Android masuk lewat JNI, jadi ada dua bagian yang harus terpasang di
`src-tauri/gen/android/`:

1. Dependensi Gradle CameraX di `app/build.gradle.kts`.
2. Izin `CAMERA` di `AndroidManifest.xml`.
3. Salin `android/CameraBridge.kt` dan `android/CameraFrameAnalyzer.kt` ke
   `app/src/main/java/org/antitimpa/antitimpa/`.
4. Panggil `CameraBridge` dari `MainActivity` (`onCreate` / `onDestroy`).

Perintah lengkap, alur frame, tabel gejala-log, dan **daftar yang belum
diverifikasi di perangkat** ada di **[`android/README.md`](android/README.md)** —
baca itu sebelum men-debug masalah kamera.

### Log kamera

```bash
adb logcat -s ANTITIMPA
```

---

## 3. Validasi tanpa build penuh

```bash
# Logika murni: Layer 2, Layer 3, QR, sintetik, JNI bridge (cepat, tanpa nokhwa)
cd src-tauri
cargo test --no-default-features --features jni-bridge --lib

# Termasuk backend kamera desktop
cargo test --features desktop-camera,jni-bridge --lib
```

Frontend:

```bash
npx tsc --noEmit
npm run build
```

Verifikasi cepat CRCs tanpa aplikasi (command `verify_payload_crc` di Rust;
sebelumnya `check_crc.py`):

```bash
cargo test --no-default-features --features jni-bridge --lib -- tampered_payload
```

---

## 4. Troubleshooting

| Gejala | Penyebab & solusi |
|---|---|
| Vite gagal start, "port 1420 already in use" | Disengaja (`strictPort: true`). Hentikan proses yang memakai port itu; jangan pindahkan port, karena `tauri.conf.json` menunjuk `1420`. |
| `UnsatisfiedLinkError` saat kamera dipanggil (release saja) | Simbol JNI ter-strip. Pastikan tiap simbol ada di daftar `--undefined` di `src-tauri/build.rs`, lalu build ulang lewat `./build-apk.sh` (skrip memverifikasi ini). |
| Kamera tidak terdeteksi di desktop | Aplikasi otomatis beralih ke backend **sintetik**; panel kamera menandainya dengan chip "simulasi". Kamera mungkin ada di `/dev/video1`, bukan `/dev/video0`. |
| Link Rust kehabisan disk | `cargo clean` di `src-tauri/`, atau pakai `--no-default-features` untuk sekadar menguji logika. |
