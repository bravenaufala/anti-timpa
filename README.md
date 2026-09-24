# Anti Timpa QRIS Scanner

Scanner keamanan QRIS tiga-lapis yang berjalan **100% lokal di perangkat** —
tanpa server, tanpa cloud, tanpa unggah gambar.

| Lapisan | Yang diperiksa | Implementasi |
|---|---|---|
| **Layer 1** — optik | Tamper fisik: edge density pada margin + variansi glare antar-frame | Rust (`src-tauri/src/layer1_optical.rs`) |
| **Layer 2** — EMVCo | Struktur TLV, CRC-16/CCITT-FALSE, format/mata uang/negara, konteks QR, MCC palsu | Rust (`src-tauri/src/layer2_emvco.rs`) |
| **Layer 3** — geofence | Klasifikasi kelayakan lokasi: kota klien (GPS) vs kota merchant (Tag 60), dengan jarak | Rust (`src-tauri/src/layer3_geofence.rs`) |

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
│       ├── CoverageBanner.tsx# ★ peringatan pemeriksaan sebagian
│       ├── FindingsList.tsx  # ★ temuan bernama
│       ├── ReportPanel.tsx   # ★ pembuatan laporan
│       ├── HistoryPanel.tsx  # ★ riwayat + verifikasi rantai
│       ├── ImageImportPanel.tsx # ★ analisis dari berkas gambar
│       ├── CameraPanel.tsx   # kontrol kamera + status backend
│       ├── CameraPreview.tsx # pratinjau langsung (JPEG lewat IPC)
│       └── DetailPanel.tsx   # rincian per-layer + payload
├── location.ts               # ★ resolusi lokasi per platform
├── src-tauri/                # Rust (backend)
│   ├── Cargo.toml
│   ├── tauri.conf.json       # devUrl = http://localhost:1420
│   ├── capabilities/default.json
│   ├── src/
│   │   ├── main.rs           # wrapper tipis
│   │   ├── lib.rs            # command Tauri + orkestrasi skor + findings
│   │   ├── layer1_optical.rs # ★ analisis tamper optik (Sobel + glare)
│   │   ├── layer2_emvco.rs   # parser TLV + CRC + 4 aturan risiko
│   │   ├── layer3_geofence.rs# ★ klasifikasi kelayakan lokasi
│   │   ├── geo_table.rs      # ★ koordinat kota offline (lokasi desktop)
│   │   ├── image_import.rs   # jalan validasi/demo: foto, bukan kamera
│   │   ├── history.rs        # riwayat scan + hash chain
│   │   ├── report.rs         # laporan bukti (teks + HTML)
│   │   ├── qr.rs             # decode QR (rqrr)
│   │   └── camera/
│   │       ├── mod.rs        # trait CameraBackend + tipe Frame
│   │       ├── desktop.rs    # nokhwa (V4L2/AVFoundation/MSMF)
│   │       ├── mobile/       # penerima frame dari plugin native
│   │       ├── preview.rs    # downscale + JPEG untuk pratinjau
│   │       ├── log.rs        # log tag ANTITIMPA (UI + logcat)
│   │       └── synthetic.rs  # fallback deterministik, bisa diuji
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

# Logika murni (Layer 1, Layer 2, Layer 3, riwayat, laporan, JNI bridge)
# `qr-encode` melinkinkan encoder QR dev-only sehingga backend sintetik bisa
# menggambar simbol QR sungguhan (bukan sekadar bentuk geometris).
cargo test --no-default-features --features jni-bridge --lib
cargo test --no-default-features --features "jni-bridge,qr-encode" --lib

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

## Lokasi (Layer 3)

Layer 3 membutuhkan posisi. Sumbernya berbeda per platform, dan itu disengaja:

| Platform | Sumber | Catatan |
|---|---|---|
| Android / iOS | Plugin `tauri-plugin-geolocation` | Prompt izin muncul saat scan, bukan saat aplikasi dibuka |
| Desktop | `geo_table.rs`, dari nama kota yang diketik | Desktop tidak punya layanan lokasi sistem |
| Browser (`npm run dev`) | `navigator.geolocation` | Hanya untuk kerja UI; bukan jalur yang didukung |

Geolokasi berbasis IP **tidak** dipakai: itu akan mengirim posisi pengguna ke
pihak ketiga, yang bertentangan dengan janji "tidak ada data keluar". Perbandingan
Layer 3 hanya butuh resolusi tingkat kota, bukan koordinat presisi, jadi mengetik
satu nama kota lebih murah daripada kebocoran privasi.

Untuk build Android, plugin harus diaktifkan eksplisit:

```bash
npx tauri android build --apk --features geolocation
```

`build-apk.sh` sudah menyertakannya, dan izinnya ada di
`src-tauri/capabilities/mobile.json`. Izin lokasi sengaja **tidak** di-grant
otomatis oleh skrip pasang — supaya prompt sebenarnya tetap teruji.

## Ringkasan Skor Risiko

- **LOW RISK** (< 0.35 & CRC valid)
- **CAUTION** (0.35–0.70)
- **HIGH RISK** (> 0.70 atau CRC gagal — veto keras)

Skor gabungan adalah `max(l1, l2, l3)`, dengan **veto keras** menjadi `1.0` bila
CRC gagal. Aturan band ini ada di `risk_band()` (`src-tauri/src/lib.rs`).

Yang penting untuk tidak disalahpahami: **LOW RISK berarti tidak ada anomali yang
terdeteksi, bukan jaminan QRIS sah.** Karena satu QR yang ditempeli stiker
memiliki payload yang identik dengan aslinya, hasil `LOW RISK` dari pemindaian
yang tidak menjalankan Layer 1 sama sekali tidak mencakup pemeriksaan penempelan
fisik. `ScanSnapshot.coverage` menyatakan ini secara eksplisit, dan UI
menampilkannya sebagai peringatan.

Layer 3 **tidak bisa** menghasilkan HIGH RISK sendiri. Mismatch kota adalah
sinyal lemah dan dibatasi di 0.65 (`DIFFERENT_CITY_DISTANT`), karena aturan lama
yang memberi 1.0 pada setiap mismatch membuat setiap pelanggan yang sedang di
luar kota melihat vonis merah.

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

1. **Kalibrasi threshold dengan foto asli.** Semua ambang Layer 1 di-fit pada
   fixture sintetik. Jalur *import gambar* (`analyze_image_bytes`) ada justru
   untuk menutup celah ini: cetak QRIS, tempel overlay, foto di beberapa kondisi
   cahaya, impor, lalu fit ulang empat konstanta di `layer1_optical.rs`.
   Metrik mentah (`spatial_edge_density`, `glare_fraction`, dll.) sudah
   dilaporkan di `Layer1Result`, jadi re-fit tidak butuh ubah struktur kode.
   **Ini prioritas tertinggi dan risiko terbesar yang tersisa.**
2. **Detektor belum pernah diuji terhadap foto overlay sungguhan.** Semua hasil
   di README ini berasal dari fixture yang dirender. Satu sore dengan printer dan
   beberapa ponsel akan menjawabnya.
3. **Tabel kota** — dua tabel: `layer3_geofence::CITIES` (~12 titik rujukan
   untuk penilaian jarak) dan `geo_table::SEED` (~38 kota untuk geocoding offline
   dari nama yang diketik). Keduanya benih, belum dataset lengkap ~514
   kabupaten/kota. Ini tugas data, bukan perubahan logika.
4. **Jembatan kamera mobile di perangkat.** Sisi Rust
   (`camera/mobile/android.rs`) sudah diuji di host lewat fitur `jni-bridge`, tapi
   `CameraBridge.kt` belum pernah dikompilasi/dijalankan di perangkat nyata.
   Lihat [`android/README.md`](android/README.md).
5. **Lokasi di Android belum diuji di perangkat.** Plugin geolocation sudah
   terpasang dan izinnya dideklarasikan di `capabilities/mobile.json`, tapi
   prompt izin dan pembacaan posisi asli belum pernah diverifikasi di perangkat.
6. **Kamera iOS.** Belum ada bridge AVFoundation.
7. **EXIF orientation** pada gambar impor belum ditangani; foto potret dari
   beberapa ponsel bisa masuk dalam keadaan terotasi.
8. **`src-tauri/gen/`** — dibuat ulang oleh `npx tauri android init`.
   `build-apk.sh` dan langkah pemasangan `CameraBridge.kt` bergantung padanya.
