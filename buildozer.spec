[app]

# (str) Title of your application
title = Anti Timpa QRIS Scanner

# (str) Package name that will be used to identify your app
package.name = antitimpa

# (str) Package domain (needed for Android/Play Store publishing)
package.domain = org.antitimpa

# (str) Source code where the main.py live
source.dir = app

# (list) Source files to exclude
source.exclude_exts = spec
source.exclude_dirs = tests, bin, .git, __pycache__

# (str) Application versioning of the generated APK
version = 0.1.0

# (list) Application requirements (order matters).
# DUAL-LAYER + kamera nyata: pakai recipe NATIVE p4a `opencv` (4.5.1) -- BUKAN
# pip `opencv-python`. Alasan: pip opencv-python ter-bundle oleh p4a sebagai
# versi 5.x yang TIDAK menyertakan file `config.py`, sehingga import cv2 di
# Android gagal (ImportError: missing configuration file ['config.py']) dan app
# langsung crash. Recipe native `opencv` memakai OPENCV_SKIP_PYTHON_LOADER=ON
# sehingga cv2.so dimuat langsung tanpa `config.py` -> Layer 1 optik bekerja.
requirements = python3,kivy,kivymd==1.2.0,pillow,opencv,numpy,camera4kivy,gestures4kivy,plyer,pyzbar

# Entry point script. Catatan: Kivy di Android SELALU mengeksekusi `main.py`
# (bukan file ini). Karena itu `app/main.py` kini adalah dispatcher yang
# mengarahkan ke `main_mobile` di Android dan `main_desktop` di desktop.
source.main = main.py

# Kamera Android via Camera4Kivy: gunakan camerax_provider sebagai p4a hook
# supaya Gradle mendapat dependensi CameraX + izin CAMERA + source Java-nya.
# Folder camerax_provider/ berada di root proyek (bukan bagian source.app).
p4a.hook = camerax_provider/gradle_options.py

# (str) Default orientation (landscape | portrait | portrait-reverse | landscape-reverse | all)
# Use fullSensor (below) so the screen follows the device rotation.
orientation = portrait
android.manifest.orientation = fullSensor

# (bool) Indicate if the application should be fullscreen
fullscreen = 0

# (int) Android API level to use (targetSdk)
android.api = 34
# (int) Minimum Android API to support
android.minapi = 21

# (bool) Allow debug keystore (default True for debug APK)
android.debug = True

# (ist) Architectures to build for. arm64-v8a is fastest; add armeabi-v7a for
# older 32-bit devices.
android.archs = arm64-v8a

# Reuse the SDK that is already installed on this machine so buildozer does
# not download a fresh SDK + NDK. Adjust the NDK path to your installed one.
android.sdk_path = /home/ahza/Android/Sdk
# Use NDK 25b (recommended by p4a v2024). The newer NDK 27 causes
# `clang: -print-multi-os-directory` errors when building sqlite3 via ndk-build.
android.ndk_path = /home/ahza/Android/Sdk/ndk/25.2.9519653

# Path to apache-ant used by the build (buildozer already prepared it).
android.build_tools_version = 35.0.0

# (bool) Added to package to allow access to the camera hardware
# READ_MEDIA_IMAGES dibutuhkan untuk membaca gambar galeri di Android 13+.
android.permissions = CAMERA, READ_EXTERNAL_STORAGE, WRITE_EXTERNAL_STORAGE, READ_MEDIA_IMAGES

# (str) Icon for the application
icon.filename = %(source.dir)s/icons/icon.png

# (str) Presplash color
presplash.color = #12344F

# (str) Buildozer log level
log_level = 2

# (str) python-for-android branch/tag.
# We use a local fork (p4a-fork/) so our patch to the 'jpeg' recipe survives
# across runs (buildozer otherwise re-clones and overwrites local edits).
# The fork is pinned to v2024.01.21, which builds Python 3.11.5 (stable).
p4a.source_dir = /home/ahza/Documents/code-projects/anti-timpa/p4a-fork
#p4a.branch = v2024.01.21

[buildozer]

# (int) Log level for buildozer (0=error, 3=debug)
log_level = 2

# (bool) Glob pattern match to warn on copy of sensitive files
warn_on_root = 1
