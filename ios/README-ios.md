# Anti Timpa QRIS Scanner — iOS Build Notes (Kivy-iOS)

iOS packaging of this Kivy+OpenCV app is **only possible on a macOS machine
with Xcode** installed. You cannot build an IPA from Windows/Linux.

## Prerequisites (macOS only)
- macOS (Big Sur or newer)
- Xcode + Command Line Tools
- Homebrew
- Python 3.x with `pip`

## Steps
1. Install Kivy-iOS toolchain:
   ```bash
   python3 -m pip install kivy-ios
   toolchain build python3 kivy kivymd numpy opencv pillow plyer
   ```
2. Build the Xcode project:
   ```bash
   toolchain create AntiTimpa app
   toolchain build AntiTimpa
   toolchain link AntiTimpa <ios-deploy|simulator>
   ```
3. Open the generated `.xcodeproj` in Xcode, sign with your Apple Developer
   team, and run on a device/simulator.

## Camera note
iOS does not expose raw numpy frames to Python through Kivy's default camera.
The current app uses the built-in **synthetic demo mode** on mobile so the full
dual-layer pipeline runs locally. To feed real iOS camera frames into OpenCV,
you need a camera bridge (e.g. a native bridge via `pyobjus` / `camera4kivy`).
See the README "Roadmap" section.

## Fully local guarantee
All Layer 1 (optics) and Layer 2 (EMVCo) analysis runs on-device. No network
permissions are added, so no data leaves the phone.
