# Anti Timpa QRIS Scanner

Scanner keamanan QRIS tiga-lapis yang berjalan **100% lokal di perangkat** —
tanpa server, tanpa cloud, tanpa unggah gambar.

| Lapisan | Yang diperiksa | Implementasi |
|---|---|---|
| **Layer 1** — optik | Tamper fisik: edge density pada quiet zone + variansi glare antar-frame | Rust (`src-tauri/src/camera/synthetic.rs`, `lib.rs`) |
| **Layer 2** — EMVCo | Struktur TLV, CRC-16/CCITT-FALSE, format/mata uang/negara, konteks QR, MCC palsu | Rust (`src-tauri/src/layer2_emvco.rs`) |
| **Layer 3** — geofence | Kota klien (GPS) vs kota merchant (Tag 60) | Rust (`src-tauri/src/layer3_geofence.rs`) |

Aplikasi tersedia untuk **desktop** (Linux/macOS/Windows), **Android**, dan
**iOS** dari satu basis kode yang sama.

> **Catatan migrasi:** proyek ini sebelumnya adalah aplikasi Python/KivyMD.
> Versi Python sudah dihapus. Lihat [Migrasi dari Python](#migrasi-dari-python)
> untuk apa yang berubah dan apa yang belum diporting.

## Struktur

```
anti-timpa/
├── src/                      # React (UI)
│   ├── main.tsx
│   ├── App.tsx               # layar utama + form payload + kamera
│   ├── api.ts                # satu-satunya jembatan ke Rust (invoke)
│   ├── types.ts              # tipe bersama, cerminan struct Rust
│   ├── samples.ts            # fixture demo
│   ├── styles.css
│   └── components/
│       ├── RiskGauge.tsx     # gauge skor gabungan
│       ├── CameraPanel.tsx   # kontrol kamera + status backend
│       ├── CameraPreview.tsx # pratinjau langsung (JPEG lewat IPC)
│       └── DetailPanel.tsx   # rincian per-layer + payload
├── src-tauri/                # Rust (backend)
│   ├── Cargo.toml
│   ├── tauri.conf.json       # devUrl = http://localhost:1420
│   ├── capabilities/default.json
│   └── src/
│       ├── main.rs           # wrapper tipis
│       ├── lib.rs            # command Tauri + orkestrasi skor + blur
│       ├── layer2_emvco.rs   # parser TLV + CRC + 4 aturan risiko
│       ├── layer3_geofence.rs# pencocokan kota
│       ├── qr.rs             # decode QR (rqrr), ganti cv2 + pyzbar
│       └── camera/
│           ├── mod.rs        # trait CameraBackend + tipe Frame
│           ├── desktop.rs    # nokhwa (V4L2/AVFoundation/MSMF)
│           ├── mobile/       # penerima frame dari plugin native
│           ├── preview.rs    # downscale + JPEG untuk pratinjau
│           ├── log.rs        # log tag ANTITIMPA (UI + logcat)
│           └── synthetic.rs  # fallback deterministik, bisa diuji
├── android/                  # separuh Kotlin dari jembatan kamera CameraX
├── build-apk.sh              # build + sign + verifikasi simbol JNI
├── keystore/                 # keystore rilis Android
└── README.md
```

## Menjalankan

```bash
npm install

# Hanya UI di browser (backend Rust tidak aktif; ada peringatan di layar)
npm run dev

# Aplikasi penuh: React + Rust core
npm run tauri:dev

# Build produksi
npm run tauri:build
```

Dev server Vite dikunci ke `http://localhost:1420` dengan `strictPort: true`,
dan `tauri.conf.json` menunjuk `devUrl` ke alamat yang sama. Kalau port itu
terpakai, Vite gagal keras alih-alih diam-diam pindah port — supaya WebView
tidak pernah menampilkan halaman kosong.

## Validasi

```bash
cd src-tauri

# Logika murni (Layer 2, Layer 3, QR, sintetik, JNI bridge)
cargo test --no-default-features --features jni-bridge --lib

# Termasuk backend kamera desktop
cargo test --features desktop-camera,jni-bridge --lib
```

Frontend:

```bash
npx tsc --noEmit
npm run build
```

## Build Android (APK)

```bash
./build-apk.sh              # build + sign
./build-apk.sh --install    # build + sign + install ke perangkat
```

Skrip ini memverifikasi simbol JNI masih ada di `.so` rilis sebelum menandatangani
APK — release build memakai LTO + `strip = true`, yang menghapus entry point JNI
kecuali `build.rs` memaksanya tetap ada. Tanpa pemeriksaan ini, kegagalan hanya
muncul di perangkat sebagai `UnsatisfiedLinkError` saat kamera pertama dipanggil.

Detail lengkap (alur JNI, bug yang pernah ditemukan, catatan izin kamera) ada di
[`android/README.md`](android/README.md).

Prasyarat: Android SDK + NDK, keystore di `keystore/antitimpa.keystore`
(lihat `build-apk.sh` untuk variabel yang bisa di-override).

## Konfigurasi localhost / jaringan

- `vite.config.ts` → `server.host` default `127.0.0.1`, port `1420`.
- Untuk uji dari perangkat fisik, set `TAURI_DEV_HOST=<ip-lan>`; Vite akan
  listen di IP itu dan HMR otomatis pindah ke port `1421`.
- `tauri.conf.json` → CSP sudah ketat: `default-src 'self'`, hanya `img-src`
  yang mengizinkan `blob:`/`data:` dan aset lokal. Tidak ada akses jaringan
  keluar, konsisten dengan janji "100% lokal".

## Ringkasan Skor Risiko

- **LOW RISK** (< 0.35 & CRC valid)
- **CAUTION** (0.35–0.70)
- **HIGH RISK** (> 0.70 atau CRC gagal — veto keras)

Skor gabungan adalah `max(l1, l2, l3)`, dengan **veto keras** menjadi `1.0` bila
CRC gagal. Aturan band ini ada di `risk_band()` (`src-tauri/src/lib.rs`).

## Log detail scan

Semua log memakai tag `ANTITIMPA`, sama seperti aplikasi lama, sehingga alat
diagnostik yang sudah ada tetap berlaku:

```bash
adb logcat -s ANTITIMPA
```

## Migrasi dari Python

Versi Python/KivyMD (`app/`, `layer*.py`, `main_*.py`, `buildozer.spec`,
`camerax_provider/`, `p4a-fork/`, `bin/`, `.buildozer/`, `ios/`, dan venv) sudah
dihapus. Yang setara di Rust:

| Python lama | Padanan sekarang | Status |
|---|---|---|
| `layer2_emvco.py` | `src-tauri/src/layer2_emvco.rs` | Selaras; fixture `test_layer2.py` di-inline sebagai unit test Rust |
| `layer3_geofence.py` | `src-tauri/src/layer3_geofence.rs` | Selaras |
| `scanner_core.py` (detect QR) | `src-tauri/src/qr.rs` (`rqrr`) | Menggantikan `cv2` + `pyzbar` |
| `scanner_core.py` (blur gate) | `lib.rs` (Laplacian variance) | Selaras, kernel 3x3 sama |
| `scanner_core.py` (skor gabungan) | `lib.rs` (`risk_band`) | Selaras |
| `main_mobile.py` / `main_desktop.py` (UI) | `src/App.tsx` + komponen React | Diganti |
| `main_mobile.py` (generator sintetik) | `camera/synthetic.rs` | Selaras; geometri mengikuti `test_layer1.py` |
| `main_desktop.py` (`cv2.VideoCapture`) | `camera/desktop.rs` (`nokhwa`) | Diganti |
| `test_layer1.py` / `test_layer2.py` | unit test Rust di `src-tauri/src` | Diganti |
| `check_crc.py` | `verify_payload_crc` (command Tauri) | Diganti |
| `camerax_provider/` (Camera4Kivy) | `android/CameraBridge.kt` + `camera/mobile/` | Diganti; **belum diuji di perangkat** |
| `live_scanner.py` (skrip konsol) | dibuang — digantikan UI | — |

Verifikasi silang yang sudah dilakukan (payload identik, Python vs Rust):

| Pemeriksaan | Python | Rust |
|---|---|---|
| CRC payload valid | `1B52` | `1B52` |
| `l2_score` (QRIS bersih) | `0.0` | `0.0` |
| `initiation_mode` | `11` | `11` |
| `mcc` | `5411` | `5411` |
| Geofence Bandung vs JAKARTA | `1.0` / HIGH RISK | `1.0` / HIGH RISK |

> Catatan pengembangan yang lebih mendetail — prototipe kamera, temuan bug JNI,
> dan catatan lingkungan saat kedua versi masih berdampingan — ada di
> [`DEVNOTES.md`](DEVNOTES.md).

## Yang belum selesai

1. **Layer 1 optik penuh** — jalur *import gambar* dan analisis edge-density
   per-frame belum diporting; saat ini Layer 1 memakai frame sintetik, dan
   `analyze_payload` memakai placeholder `l1_score = 0.0` (ditandai
   `"risk_level": "NOT RUN"`). Ambang `0.15` dan `0.003` dari Python sangat
   sensitif, jadi portnya butuh golden test.
2. **Jembatan kamera mobile di perangkat.** Sisi Rust (`camera/mobile/android.rs`)
   sudah diuji di host lewat fitur `jni-bridge`, tapi `CameraBridge.kt` belum
   pernah dikompilasi/dijalankan di perangkat nyata. Lihat
   [`android/README.md`](android/README.md).
3. **Kamera iOS.** Belum ada bridge AVFoundation.
4. **`src-tauri/gen/`** — dibuat ulang oleh `npx tauri android init`.
   `build-apk.sh` dan langkah pemasangan `CameraBridge.kt` bergantung padanya.
