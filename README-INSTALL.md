# Panduan Instalasi — Anti Timpa QRIS Scanner

Panduan praktis mem-build & menjalankan aplikasi di **Desktop**, **Android**, dan
**iOS**. Semua analisis berjalan **lokal di perangkat** — tanpa server, tanpa
cloud. Aplikasi dapat **scan berkali-kali** dan risiko/merchant berubah setiap
kali kamera menunjuk QR.

> Versi lama aplikasi ini adalah Python/KivyMD. **Kode Python sudah dihapus**;
> sekarang React + Tauri + Rust (lihat [`README.md`](README.md)). Dokumen ini
> sudah disesuaikan.

---

## 🖥️ Desktop (Windows / macOS / Linux)

**Persyaratan:** Node.js 18+, Rust 1.77.2+, webcam. Di Linux juga butuh
`libwebkit2gtk-4.1-dev` dan `libv4l-dev`.

```bash
# 1) Pasang dependensi frontend + Tauri CLI (lokal, di node_modules)
npm install

# 2) Jalankan aplikasi penuh (React + Rust)
npm run tauri:dev
```

Atau langsung tanpa CLI global:

```bash
npx tauri dev
```

> Jika webcam tidak ada, app otomatis masuk **mode demo sintetik**; panel kamera
> menandainya dengan chip "simulasi". Kamera desktop tidak selalu di `/dev/video0`
> — lihat bagian "Pratinjau Kamera Langsung" di `README.md`.

---

## 🤖 Android (APK)

**Persyaratan:** Android SDK + NDK, keystore rilis, Node.js + Rust.

```bash
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/<versi>"

# Build + tanda tangani
./build-apk.sh

# Build + sign + pasang ke perangkat yang terhubung
./build-apk.sh --install
```

Hasil: **`dist-apk/antitimpa-release.apk`** (~8–9 MB).

`build-apk.sh` sengaja memverifikasi simbol JNI di `.so` rilis **sebelum**
menandatangani, karena build release memakai LTO + `strip = true` yang menghapus
entry point JNI bila `build.rs` tidak memaksanya tetap ada. Kegagalannya hanya
akan terlihat di perangkat sebagai `UnsatisfiedLinkError` saat kamera pertama
dipanggil.

### 3. Memasang jembatan kamera CameraX

Frame kamera Android masuk lewat JNI. Pasang di `src-tauri/gen/android/`:

1. dependensi CameraX di `app/build.gradle.kts`,
2. izin `CAMERA` di `AndroidManifest.xml`,
3. salin `android/CameraBridge.kt` + `android/CameraFrameAnalyzer.kt` ke
   `app/src/main/java/org/antitimpa/antitimpa/`,
4. panggil `CameraBridge` dari `MainActivity`.

Langkah lengkap + tabel diagnostik ada di
**[`android/README.md`](android/README.md)**. **Jembatan ini belum pernah
diverifikasi di perangkat nyata** — hanya sisi Rust-nya yang sudah diuji di host.

### Cek log (jika crash / error)

```bash
adb logcat -s ANTITIMPA
```

Tag `ANTITIMPA` sengaja dipertahankan sama seperti aplikasi lama, sehingga
kebiasaan dan alat diagnostik yang sudah ada tetap berlaku.

---

## 🍎 iOS (IPA)

Bisa dibangun dari mana saja dengan `npx tauri ios build`, tetapi:

> **Catatan kamera iOS:** belum ada bridge AVFoundation, jadi kamera iOS belum
> berfungsi. Layer 2 + Layer 3 tetap berjalan lokal di perangkat, dan Layer 1
> memakai backend sintetik.

---

## 🔍 Verifikasi cepat (tanpa build penuh)

```bash
# Logika murni: Layer 2, Layer 3, QR, sintetik, JNI bridge
# (--no-default-features melewatkan nokhwa/V4L2 sehingga cepat & hemat disk)
cd src-tauri
cargo test --no-default-features --features jni-bridge --lib

# Frontend
cd ..
npx tsc --noEmit
npm run build
```

Pemeriksaan CRC payload (dulu `check_crc.py`) sekarang jadi command
`verify_payload_crc` di Rust:

```bash
cd src-tauri
cargo test --no-default-features --features jni-bridge --lib -- tampered_payload
```

## 📦 Paket yang dipakai per platform

| Kebutuhan | Desktop | Android | iOS |
|---|---|---|---|
| UI (React + Vite) | ✅ | ✅ | ✅ |
| Tauri + Rust core | ✅ | ✅ | ✅ |
| Kamera | `nokhwa` (V4L2/AVFoundation/MSMF) | CameraX via JNI | ❌ belum ada bridge |
| Decode QR | `rqrr` | `rqrr` | `rqrr` |
| Layer 1 optik | frame sintetik | frame sintetik | frame sintetik |
| Layer 2 EMVCo + CRC | ✅ | ✅ | ✅ |
| Layer 3 geofence | ✅ (kota manual) | ✅ | ✅ |

## 🚀 Menjalankan (mode utama)

- **Live scan:** arahkan kamera ke QRIS → risiko & merchant muncul.
- **Input payload manual:** tempel string QRIS lalu tekan Analisis — berguna
  untuk menguji tiap aturan risiko tanpa kamera (lihat chip contoh payload).
