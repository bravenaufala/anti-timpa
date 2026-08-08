#!/usr/bin/env bash
# Anti Timpa QRIS Scanner - cross-platform launcher (desktop)
# Usage: ./run_desktop.sh   (optionally: CAMERA_SOURCE=2 ./run_desktop.sh)
set -e
cd "$(dirname "$0")"
exec python3 app/main.py
