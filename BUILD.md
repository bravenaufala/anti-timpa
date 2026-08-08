# Panduan Build — Anti Timpa QRIS Scanner

Aplikasi lintas-platform (desktop / Android / iOS) berbasis **KivyMD** yang
menjalankan analisis QRIS **100% lokal per perangkat**.

> 📥 **Panduan install & command singkat per platform**: lihat
> **[`README-INSTALL.md`](README-INSTALL.md)**.

| Platform | Entry point | Framework |
|---|---|---|
| **Desktop** (Win/macOS/Linux) | `app/main_desktop.py` | KivyMD + OpenCV (webcam) |
| **Android** (APK) | `app/main.py` → `main_mobile.py` (L1+L2) | KivyMD (Buildozer) |
| **iOS** (IPA) | `app/main.py` → `main_mobile.py` (L1+L2) | Kivy-iOS (butuh macOS + Xcode) |

---

## 1. Build Android APK

`buildozer.spec` sudah siap. Berikut langkah yang **terbukti berhasil** di
mesin Linux (toolchain yang sudah terpasang tertulis di bawah).

### A. Prasyarat toolchain

Toolchain yang dibutuhkan (sudah tersedia di mesin, versi teruji):

| Tool | Versi teruji | Keterangan |
|---|---|---|
| Python | **3.12.x** | Buildozer/p4a **tidak** mendukung 3.13/3.14 |
| Buildozer | 1.6.0 | instal di venv Python 3.12 |
| cmake | 4.4.2 | dibutuhkan recipe `jpeg`/libjpeg-turbo |
| ninja | 1.13.2 | pendamping cmake |
| autoconf, automake, libtool, pkg-config | 2.73 / 1.18 / 2.6 | dibutuhkan `libffi` |
| **Java 17** | 17.0.20 | **wajib** — Gradle 8.0.2 tidak menerima Java 21 |
| Android SDK | platform 34, build-tools 35 | via sdkmanager |
| Android NDK | **25.2.9519653** | rekomendasi p4a v2024 |

> ⚠️ **Jangan pakai Python 3.13/3.14.** Buildozer + Kivy belum punya wheel, dan
> p4a gagal membangun CPython 3.14.

> ⚠️ **Gunakan Java 17**, bukan Java 21. Gradle 8.0.2 yang dipakai p4a menolak
> Java 21 dengan error `Unsupported class file major version 65`.

> ⚠️ **Gunakan NDK 25.2.9519653**, bukan NDK 27. NDK 27 menyebabkan
> `clang: -print-multi-os-directory` saat build `sqlite3`.

### B. Persiapkan environment build

```bash
cd anti-timpa

# 1) Python 3.12 + venv (di mesin ini sudah ada .venv-android)
python3.12 -m venv .venv-android
.venv-android/bin/pip install --upgrade buildozer

# 2) Tambahkan ke ~/.bashrc agar selalu aktif saat build
#    (VIRTUAL_ENV HARUS menunjuk .venv-android, jika tidak buildozer salah
#    mengenali venv dan memakai pip --user yang gagal di Python 3.12 venv)
export VIRTUAL_ENV="$PWD/.venv-android"
export PATH="$VIRTUAL_ENV/bin:/home/linuxbrew/.linuxbrew/opt/openjdk@17/bin:$PATH"
export JAVA_HOME="/home/linuxbrew/.linuxbrew/opt/openjdk@17"
export ANDROID_SDK_ROOT="$HOME/Android/Sdk"
export ANDROID_HOME="$HOME/Android/Sdk"
```

> ⚠️ **Modul analisis harus berada di dalam `app/` (source.dir).** Buildozer hanya
> mengemas isi `source.dir` (default `app`). Jadi `layer1_optical.py` DAN
> `layer2_emvco.py` harus SALIN ke `app/` (keduanya sudah tersalin). Kalau
> aplikasi **auto-close saat dibuka**, penyebab paling umum adalah modul yang
> di-import tidak ikut ter-paket
> → cek `private.tar`: `unzip -p bin/*.apk assets/private.tar > p && tar -tf p`
> harus berisi `layer1_optical.pyc` dan `layer2_emvco.pyc`.

### B2. Provider kamera Camera4Kivy (camerax_provider)

`main_mobile.py` memakai **Camera4Kivy** untuk membuka kamera Android (CameraX)
yang frame-nya dianalisis oleh Layer 1 optik. Jangan hapus folder
**`camerax_provider/`** di root — folder ini dicari oleh `p4a.hook =
camerax_provider/gradle_options.py` untuk menambahkan dependensi Gradle
CameraX + izin CAMERA + source Java-nya ke build.

### C. Bangun APK

```bash
source ~/.bashrc        # memuat env di atas
cd anti-timpa
buildozer -v android debug
```

Hasil: **`bin/antitimpa-0.1.0-arm64-v8a-debug.apk`** (≈100 MB, sudah berisi
OpenCV + numpy + camera4kivy sehingga Layer 1 optik aktif).

Verifikasi validitas:
```bash
"$ANDROID_SDK_ROOT/build-tools/37.0.0/aapt" dump badging bin/*.apk
# package: org.antitimpa.antitimpa | minSdk 21 | targetSdk 34 | CAMERA
```

> Build berhasil diuji di mesin ini (lihat `build_apk.log`). Langkah kunci:
> pakai **Java 17** (`export JAVA_HOME=/home/linuxbrew/.linuxbrew/opt/openjdk@17`)
> dan tambahkan `.venv-android/bin` ke `PATH` supaya `buildozer` menemukan
> `cython`.

### D. Yang dilakukan `p4a-fork/` (jangan dihapus)

`buildozer.spec` menunjuk `p4a.source_dir` ke folder **`p4a-fork/`** (p4a
`v2024.01.21` — membangun **Python 3.11.5** yang stabil). Folder ini berisi
satu patch penting pada recipe `jpeg` agar kompatibel dengan **CMake 4**:

```python
# p4a-fork/pythonforandroid/recipes/jpeg/__init__.py
'-DCMAKE_POLICY_VERSION_MINIMUM=3.5',   # libjpeg-turbo 2.0.1 vs CMake 4
```

> Alasan memakai fork: membangun sendiri hostpython3 di lingkungan hybrid
> butuh `libffi-dev` distro (`sudo apt install libffi-dev`) supaya modul host
> `_ctypes`/`pyexpat` ikut tertimbun dengan benar.

### E. Pegangan troubleshooting cepat

- **`Unsupported class file major version 65`** → ganti ke Java 17.
- **`CMake Error ... cmake_minimum_required`** → pastikan patch `jpeg` ada di
  `p4a-fork` (lihat D).
- **`_ctypes` / `pyexpat` host gagal** → instal `sudo apt-get install -y libffi-dev`,
  lalu bersihkan `.buildozer/android/platform/build-arm64-v8a/build/other_builds/hostpython3`
  dan ulangi build.
- **`-print-multi-os-directory` (sqlite3)** → gunakan NDK 25.2.9519653.

### F. App langsung keluar (auto-close) / ingin log error

`main_mobile.py` kini menulis log error lengkap ke **`antitimpa.log`** di
penyimpanan aplikasi (app private storage), jadi walau app tertutup cepat,
traceback-nya tersimpan. Ambil lewat `adb`:

```bash
adb shell run-as org.antitimpa.antitimpa cat files/antitimpa.log
# atau salin ke /sdcard dulu
adb shell run-as org.antitimpa.antitimpa cp files/antitimpa.log /sdcard/
adb pull /sdcard/antitimpa.log .
```

Sebaiknya kirim file **`antitimpa.log`** itu saat melaporkan masalah. Penyebab
auto-close yang paling sering dan cara cek:
- **Modul tidak ter-paket** → pastikan `private.tar` berisi `layer1_optical.pyc`
  & `layer2_emvco.pyc` (lihat catatan ⚠️ di bagian B).
- **Native lib OpenCV/numpy gagal dimuat** (`ImportError: lib...so` /
  `undefined symbol`) → muncul di log sebagai `IMPORT ERROR (dua-layer tidak
  aktif)`. Saat ini app tetap jalan di mode Layer-2 (tidak langsung keluar).
- **Error di dalam Kivy `build()` / top-level** → tertulis sebagai
  `UNCAUGHT EXCEPTION` / `TOP-LEVEL CRASH`.
- Error per-frame kamera → `FRAME ERROR`, tidak mematikan app.

---

## 2. Build iOS (IPA)

Hanya bisa di **macOS + Xcode**. Lihat `ios/README-ios.md` untuk detail.

Ringkas:
```bash
brew install pkg-config sdl2 sdl2_image sdl2_mixer sdl2_ttf
python3 -m pip install kivy-ios
# siapkan aplikasi Kivy di folder terpisah bernama ios/app (misal)
mkdir -p ios/app && cp app/*.py ios/app/
toolchain build python3 kivy kivymd numpy pillow
# buat project Xcode bernama AntiTimpa dari source di ios/app
toolchain create AntiTimpa $PWD/ios/app
toolchain build AntiTimpa   # hasil: ios/AntiTimpa-ios/ xcodeproj
open ios/AntiTimpa-ios/AntiTimpa.xcodeproj   # sign & run di Xcode
```

> Catatan: build Android/iOS kini memakai `app/main_mobile.py` (dual-layer).
> Layer 1 optik aktif karena `buildozer.spec` menyertakan `opencv-python` +
> `numpy`; feed memakai generator sintetik sampai bridge kamera tersedia.
> Bila build OpenCV bermasalah, hapus `opencv-python,numpy` dari
> `requirements` di `buildozer.spec` supaya otomatis jadi Layer-2-only.

---

## 3. Build / Jalankan Desktop

### Linux
```bash
python3 -m venv .venv && source .venv/bin/activate
pip install -r app/requirements-desktop.txt
python app/main.py            # atau ./run_desktop.sh
```
Kamera default index `0`; atur dengan `CAMERA_SOURCE=2`.

### macOS / Windows
Sama seperti di atas — KivyMD + OpenCV berjalan native. Pastikan Python
**3.10–3.12** (Kivy belum punya wheel untuk 3.13+).

---

## 4. Verifikasi logika (tanpa build)

```bash
python3 test_layer1.py   # Layer 1 optik
python3 test_layer2.py   # Layer 2 EMVCo + CRC
```
