# Anti Timpa QRIS Scanner

Scanner keamanan QRIS tiga-lapis (Layer 1 optik + Layer 2 EMVCo + Layer 3
Kota geofence) yang dibungkus menjadi aplikasi **lintas-platform** (desktop /
Android / iOS) dengan **KivyMD**. Seluruh analisis berjalan **lokal di
perangkat** — tanpa server, tanpa cloud.

## Struktur

```
anti-timpa/
├── layer1_optical.py        # Analisis tamper fisik (edge density + glare) [tidak diubah]
├── layer2_emvco.py          # Validasi payload EMVCo + CRC-16 [tidak diubah]
├── layer3_geofence.py       # Geofence kota (GPS klien vs kota merchant Tag 60)
├── test_layer1.py           # Test Layer 1 (dipertahankan)
├── test_layer2.py           # Test Layer 2 (dipertahankan)
├── live_scanner.py          # Skrip desktop konsol asli (dipertahankan)
├── camerax_provider/        # Provider kamera CameraX utk Camera4Kivy (hook p4a)
├── app/
│   ├── main.py              # Aplikasi KivyMD (entry point desktop)
│   ├── main_mobile.py       # Aplikasi KivyMD mobile: Layer 1 + 2 + 3 + kamera nyata
│   ├── main_desktop.py      # Aplikasi KivyMD desktop: Layer 1 + 2 + 3
│   ├── scanner_core.py      # Mesin analisis lintas-platform (L1 + L2 + L3)
│   ├── layer1_optical.py    # Salinan layer1 (waib agar ter-paket ke APK)
│   ├── layer2_emvco.py      # Salinan layer2 (waib agar ter-paket ke APK)
│   ├── layer3_geofence.py   # Salinan layer3 (waib agar ter-paket ke APK)
│   ├── requirements-desktop.txt
│   └── requirements-mobile.txt
├── buildozer.spec           # Konfigurasi build Android (APK)
├── ios/README-ios.md        # Catatan build iOS (butuh macOS + Xcode)
└── run_desktop.sh           # Peluncur desktop
```

## Menjalankan di Desktop

```bash
python -m venv .venv && . .venv/bin/activate
pip install -r app/requirements-desktop.txt
python app/main.py
```

Atau langsung: `./run_desktop.sh`.

Kamera default index `0`. Atur lewat variabel lingkungan: 
`CAMERA_SOURCE=2 ./run_desktop.sh`

Jika kamera tidak tersedia, aplikasi otomatis beralih ke **mode simulasi**
(generator frame sintetis) sehingga seluruh pipeline tetap jalan untuk demo.

## Build & Distribusi

**Panduan build lengkap (desktop / Android APK / iOS IPA) ada di
[`BUILD.md`](BUILD.md).** Ringkasnya:

- **Android APK** — sudah jadi: `bin/antitimpa-0.1.0-arm64-v8a-debug.apk`
  (package `org.antitimpa.antitimpa`, minSdk 21 / targetSdk 34, izin
  `CAMERA` + penyimpanan). Rebuild: `buildozer -v android debug`.
- **iOS IPA** — butuh macOS + Xcode. Lihat `ios/README-ios.md`.
- **Desktop** — `./run_desktop.sh` atau `python app/main.py`.

## Lokal 100%

- Semua perhitungan (`layer1_optical` & `layer2_emvco`) memakai `numpy`/`cv2`
  di perangkat.
- Tidak ada izin jaringan yang diminta. Gambar kamu tidak pernah diunggah.

## Ringkasan Skor Risiko

- **LOW RISK** (< 0.35 & CRC valid)
- **CAUTION** (0.35–0.70)
- **HIGH RISK** (> 0.70 atau CRC gagal — veto keras)

## Layer 3 — Geofence Kota

Kota klien (dari GPS, di-reverse-geocode ke nama kota) dibandingkan dengan kota
merchant dari QR (Tag 60). Cocok → `LOW RISK`; tidak cocok → `HIGH RISK`
(anomali geofence). Jika lokasi klien atau kota merchant tidak tersedia, cek
dilewati (`LOW RISK`). Skor `l3_score` ikut dalam skor gabungan
(`max(l1, l2, l3)`), dengan veto keras CRC bila gagal.

- **Mobile**: GPS (`plyer.gps`) + reverse geocoding (Nominatim). Izin lokasi
  diminta bersama izin kamera.
- **Desktop**: tidak ada GPS — set kota klien manual lewat kotak input
  "Kota Klien" lalu tekan "Gunakan Kota".

## Mobile: dual-layer (Layer 1 + Layer 2) + kamera nyata

`app/main_mobile.py` menjalankan **Layer 1 optik DAN Layer 2 EMVCo** langsung di
perangkat. Kini memakai **Camera4Kivy** (`camera4kivy`) sebagai bridge kamera
nyata: frame kamera Android (CameraX) dianalisis oleh `QrisScannerCore` (Layer 1
edge density + glare variance, dan Layer 2 payload EMVCo + CRC). Semua analisis
berjalan **lokal** — tanpa server, tanpa jaringan.

`buildozer.spec` menyiapkan:
- `requirements` = ...,`opencv-python`,`numpy`,`camera4kivy`,`gestures4kivy`
- `p4a.hook = camerax_provider/gradle_options.py` → menambah dependensi Gradle
  CameraX + izin CAMERA + source Java provider.

Bila kamera tidak tersedia, aplikasi otomatis beralih ke **generator sintetik**
(yang menyuntikkan anomali stiker + glare) sehingga pipeline Layer 1 tetap jalan
di perangkat untuk demo. Bila OpenCV tidak ter-paket (APK ringan), turun ke
mode **Layer-2-only** (tempel payload + Analisis).

## Roadmap

- ~~Bridge kamera nyata Android (`camera4kivy`)~~ — selesai (APK debug sudah
  memakai CameraX).
- ~~Layer 3 — Geofence kota (GPS mobile / kota manual desktop)~~ — selesai
  (terhubung ke versi mobile & desktop).
- Kamera iOS ke OpenCV via bridge native (Camera4Kivy AVFoundation).
- Rilis `release` APK (bukan `debug`) + keystore untuk Play Store.
- Pilih file QRIS dari galeri pada Android/iOS (plyer `filechooser`) sudah
  disiapkan.
