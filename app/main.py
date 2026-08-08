"""
Anti Timpa QRIS - Entry point jembatan (dispatcher).

Kivy di Android SELALU mengeksekusi `main.py` sebagai titik masuk, apa pun
nilai `source.main` di buildozer.spec. Karena itu file `main.py` ini hanya
berfungsi sebagai pengalih:

  * Android (platform != 'desktop'):  jalankan `main_mobile`  ->  aplikasi
    mobile dengan Layer 1 (optik) + Layer 2 (EMVCo) + fallback yang aman bila
    OpenCV tidak tersedia, dan logging crash.
  * Desktop:                             jalankan `main_desktop` (versi asli
    yang memakai webcam OpenCV).

Alasan: `main_desktop.py` berisi `import cv2` tanpa guard di top-level, sehingga
bila OpenCV bermasalah (misal modul `config.py` hilang di APK) app langsung
keluar. Menjadikan `main.py` sebagai dispatcher mencegah hal itu.
"""

import os
import sys
import traceback
from datetime import datetime

# ---- Make analysis modules importable regardless of CWD ----
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# Logging crash yang persisten (agar app yang crash tetap menyimpan jejak).
LOG_FILENAME = "antitimpa.log"


def _log_path():
    try:
        from android.storage import app_storage_path
        return os.path.join(app_storage_path(), LOG_FILENAME)
    except Exception:
        return os.path.join(os.getcwd(), LOG_FILENAME)


def _log_crash(kind, text, e=None, tb=None):
    try:
        with open(_log_path(), "a") as f:
            f.write("\n[%s] %s\n%s\n" % (datetime.now().isoformat(), kind, text))
            if e is not None:
                f.write("Tipe: %s\nPesan: %s\n" % (type(e).__name__, str(e)))
            if tb is not None:
                f.write("".join(traceback.format_tb(tb)))
    except Exception:
        pass
    print("%s: %s" % (kind, text), flush=True)


def main():
    is_android = False
    try:
        from kivy.utils import platform
        is_android = (platform == "android")
    except Exception:
        pass

    if is_android:
        try:
            import main_mobile
            sys.exit(main_mobile.main())
        except SystemExit:
            raise
        except Exception as _e:
            # main_mobile sudah menangani kebanyakan error; tapi jaga-jaga.
            _log_crash("DISPATCH-ANDROID ERROR", "Gagal memuat main_mobile.",
                       e=_e, tb=_e.__traceback__)
            raise
    else:
        try:
            import main_desktop
            main_desktop.main()
        except SystemExit:
            raise
        except Exception as _e:
            _log_crash("DISPATCH-DESKTOP ERROR", "Gagal memuat main_desktop.",
                       e=_e, tb=_e.__traceback__)
            raise


if __name__ == "__main__":
    main()
