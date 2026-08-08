# 📥 Panduan Instalasi Anti Timpa QRIS

Panduan praktis mem-build & menjalankan aplikasi di **Desktop**, **Android**, dan
**iOS**. Semua analisis (Layer 1 optik + Layer 2 EMVCo) berjalan **lokal di
perangkat** — tanpa server, tanpa cloud. Aplikasi dapat **scan berkali-kali**
dan risiko/merchant berubah setiap kali kamera menunjuk QR.

---

## 🖥️ Desktop (Windows / macOS / Linux)

**Persyaratan:** Python 3.10 – 3.12, webcam.

```bash
# 1) Buat venv + pasang dependensi
python -m venv .venv && source .venv/bin/activate
pip install -r app/requirements-desktop.txt

# 2) Jalankan (entry: app/main.py -> dialihkan ke main_desktop)
./run_desktop.sh
```

Atau langsung tanpa script:
```bash
python app/main.py
```

**Opsional: ganti kamera** (default index `0`):
```bash
CAMERA_SOURCE=2 ./run_desktop.sh
```

> Jika webcam tidak ada, app otomatis masuk **mode demo sintetik**.

---

## 🤖 Android (APK)

**Persyaratan:** Linux + `buildozer`, Java 17, Android SDK/NDK. Jalur ini sudah
teruji di mesin Linux (lihat daftar toolchain di `BUILD.md`).

### 1. Siapkan environment build
```bash
python3.12 -m venv .venv-android
.venv-android/bin/pip install --upgrade buildozer

# Wajib: Java 17 + venv di PATH (VIRTUAL_ENV harus .venv-android)
export VIRTUAL_ENV="$PWD/.venv-android"
export PATH="$VIRTUAL_ENV/bin:/home/linuxbrew/.linuxbrew/opt/openjdk@17/bin:$PATH"
export JAVA_HOME="/home/linuxbrew/.linuxbrew/opt/openjdk@17"
export ANDROID_SDK_ROOT="$HOME/Android/Sdk"
export ANDROID_HOME="$HOME/Android/Sdk"
```

### 2. Bangun APK
```bash
buildozer android debug
```
Hasil: **`bin/antitimpa-0.1.0-arm64-v8a-debug.apk`** (~100 MB).

### 3. Install ke HP (USB / ADB Wi-Fi)
```bash
adb install -r bin/antitimpa-0.1.0-arm64-v8a-debug.apk
# beri izin kamera
adb shell pm grant org.antitimpa.antitimpa android.permission.CAMERA
```
Buka aplikasi; pertama kali beri izin kamera/penyimpanan saat diminta.

> **Kamera nyata aktif** via Camera4Kivy (CameraX). **Import gambar** memakai
> `pyzbar`/zbar → bisa baca QR kecil di foto galeri.

### Cek log (jika crash / error)
```bash
adb shell run-as org.antitimpa.antitimpa cat files/antitimpa.log
adb logcat -d | grep -iE "python|Camera|FATAL|Traceback"
```

---

## 🍎 iOS (IPA)

**Persyaratan:** **macOS** + Xcode + Homebrew. Build iOS **tidak bisa** dari
Windows/Linux.

### 1. Pasang toolchain Kivy-iOS & dependensi
```bash
python3 -m pip install kivy-ios
toolchain build python3 kivy kivymd numpy opencv pillow plyer
```

### 2. Buat project Xcode dari folder `app/`
```bash
toolchain create AntiTimpa app
toolchain build AntiTimpa
toolchain link AntiTimpa <ios-deploy|simulator>
```

### 3. Buat IPA & install
Buka `AntiTimpa-ios/AntiTimpa.xcodeproj` di Xcode, pilih team Apple Developer,
lalu **Run** ke device/simulator, atau **Archive** → hasil `.ipa`.

> **Catatan kamera iOS:** iOS belum mengekspos frame numpy mentah ke Python
> lewat kamera Kivy bawaan; saat ini memakai **mode demo sintetik**. Untuk
> frame kamera iOS asli butuh bridge native (ada di README "Roadmap").
> Layer 1 + Layer 2 tetap berjalan lokal di device.

---

## 🔍 Verifikasi cepat (tanpa build, Linux)
```bash
python3 test_layer1.py   # Layer 1 optik
python3 test_layer2.py   # Layer 2 EMVCo + CRC
```

## 📦 Paket yang dipakai per platform
| Package | Desktop | Android | iOS |
|---|---|---|---|
| Kivy + KivyMD | ✅ | ✅ | ✅ |
| OpenCV (`opencv`) | ✅ | ✅ (native recipe) | ✅ |
| numpy | ✅ | ✅ | ✅ |
| Camera4Kivy (kamera) | — | ✅ (CameraX) | — (mode sintetik) |
| pyzbar/zbar (import) | — | ✅ | — |
| plyer (file picker) | ✅ | ✅ | — |

## 🚀 Menjalankan (mode utama)
- **Live scan:** arahkan kamera ke QRIS → risiko & merchant muncul; pindah ke
  QR lain → hasil ter-update otomatis.
- **Import dari galeri:** tombol "Import Gambar" → pilih foto QRIS dari galeri.
