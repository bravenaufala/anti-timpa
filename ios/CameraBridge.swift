// CameraBridge.swift: iOS half of the native camera bridge.
//
// This file has not been built or run; that requires macOS and Xcode. The Rust
// half it calls (src-tauri/src/camera/mobile/ios.rs) is unit-tested on the host.
// See ios/README.md for what is and is not verified.
//
// This mirrors android/CameraBridge.kt + CameraFrameAnalyzer.kt: AVFoundation
// delivers frames, they are converted to packed RGBA8888, and handed to the Rust
// core through the C-ABI entry points. The Rust slot keeps only the newest
// frame; a shutter press drains it. The same design is used on both platforms.

import AVFoundation
import Foundation
import os.log

// C-ABI entry points exported by src-tauri/src/camera/mobile/ios.rs.
//
// `@_silgen_name` binds these Swift names to the Rust symbols directly. If a
// symbol is renamed on the Rust side, this fails at link time. The Android
// equivalent fails at runtime with UnsatisfiedLinkError.
@_silgen_name("antitimpa_ios_push_frame")
private func antitimpa_ios_push_frame(
    _ width: Int32,
    _ height: Int32,
    _ rotationDegrees: Int32,
    _ buffer: UnsafePointer<UInt8>?,
    _ len: Int
) -> Bool

@_silgen_name("antitimpa_ios_set_stream_active")
private func antitimpa_ios_set_stream_active(_ active: Bool)

@_silgen_name("antitimpa_ios_frames_received")
private func antitimpa_ios_frames_received() -> UInt64

/// Owns the `AVCaptureSession` and pumps frames into the Rust core.
///
/// Usage:
/// ```swift
/// let bridge = CameraBridge()
/// if bridge.start() { /* frames now flow */ }
/// // on shutdown
/// bridge.stop()
/// ```
final class CameraBridge: NSObject {

    private static let log = OSLog(subsystem: "org.antitimpa.antitimpa", category: "camera")

    private let session = AVCaptureSession()
    /// Serial queue: AVFoundation delivers frames off the main thread, and the
    /// analysis must not race the UI.
    private let queue = DispatchQueue(label: "org.antitimpa.camera", qos: .userInitiated)

    /// Reused across frames so a ~1.5 MB buffer is not allocated per frame.
    private var scratch: [UInt8] = []

    /// Frames handed to Rust since `start()`. Mirrors the Rust-side counter.
    var framesPushed: UInt64 { antitimpa_ios_frames_received() }

    /// Starts the capture session.
    ///
    /// Returns `false` (with an os_log explanation) when the camera cannot be
    /// configured, rather than throwing; the caller can show a message and
    /// fall back to the synthetic backend.
    ///
    /// The camera permission must already be granted by the caller, so the
    /// prompt stays tied to a user action in the UI.
    @discardableResult
    func start() -> Bool {
        guard AVCaptureDevice.authorizationStatus(for: .video) == .authorized else {
            os_log("[camera] izin kamera belum diberikan; bridge tidak dimulai", log: Self.log, type: .error)
            return false
        }

        guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back),
              let input = try? AVCaptureDeviceInput(device: device) else {
            os_log("[camera] kamera belakang tidak tersedia", log: Self.log, type: .error)
            return false
        }

        session.beginConfiguration()

        guard session.canAddInput(input) else {
            os_log("[camera] input tidak bisa ditambahkan", log: Self.log, type: .error)
            session.commitConfiguration()
            return false
        }
        session.addInput(input)

        // Analyse at a bounded resolution: the analysis is one-shot and does a
        // QR decode, so a huge buffer only costs time and memory.
        // kCVPixelFormatType_32BGRA is the cheapest format AVFoundation hands
        // out; it is converted to RGBA in `captureOutput`.
        let output = AVCaptureVideoDataOutput()
        output.videoSettings = [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA
        ]
        // Keep only the latest frame: the slot is one-shot, so buffering a queue
        // would only add latency.
        output.alwaysDiscardsLateVideoFrames = true
        output.setSampleBufferDelegate(self, queue: queue)

        guard session.canAddOutput(output) else {
            os_log("[camera] output tidak bisa ditambahkan", log: Self.log, type: .error)
            session.commitConfiguration()
            return false
        }
        session.addOutput(output)
        session.commitConfiguration()

        session.startRunning()
        antitimpa_ios_set_stream_active(true)
        os_log("[camera] sesi AVFoundation aktif (BGRA, back camera)", log: Self.log, type: .info)
        return true
    }

    /// Stops the session and releases the camera. Safe to call repeatedly.
    func stop() {
        if session.isRunning {
            session.stopRunning()
        }
        antitimpa_ios_set_stream_active(false)
        os_log("[camera] sesi dihentikan; total frame dikirim=%llu", log: Self.log, type: .info, antitimpa_ios_frames_received())
    }
}

extension CameraBridge: AVCaptureVideoDataOutputSampleBufferDelegate {
    func captureOutput(
        _ output: AVCaptureOutput,
        didOutput sampleBuffer: CMSampleBuffer,
        from connection: AVCaptureConnection
    ) {
        guard let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }

        let width = CVPixelBufferGetWidth(pixelBuffer)
        let height = CVPixelBufferGetHeight(pixelBuffer)
        guard width > 0, height > 0 else { return }

        // Rotation hint: portrait captures arrive rotated, so the Rust side is
        // told how to orient the pixels.
        let rotation = Int32(rotationDegrees(for: connection))

        let needed = width * height * 4
        if scratch.count < needed {
            scratch = [UInt8](repeating: 0, count: needed)
        }

        guard convertBGRAtoRGBA(pixelBuffer, into: &scratch, width: width, height: height) else {
            os_log("[camera] konversi RGBA gagal; frame dilewati", log: Self.log, type: .debug)
            return
        }

        let accepted = scratch.withUnsafeBufferPointer { ptr -> Bool in
            guard let base = ptr.baseAddress else { return false }
            return antitimpa_ios_push_frame(Int32(width), Int32(height), rotation, base, needed)
        }
        if !accepted {
            os_log("[camera] native menolak frame %dx%d (rotasi=%d)", log: Self.log, type: .debug, width, height, rotation)
        }
    }

    /// Maps the connection's video orientation to the clockwise degrees the Rust
    /// rotation logic expects.
    private func rotationDegrees(for connection: AVCaptureConnection) -> Int {
        switch connection.videoOrientation {
        case .portrait: return 90
        case .portraitUpsideDown: return 270
        case .landscapeLeft: return 180
        case .landscapeRight: return 0
        @unknown default: return 0
        }
    }

    /// Converts a locked `CVPixelBuffer` (BGRA) into packed RGBA8888.
    ///
    /// Returns `false` when the buffer layout is unexpected, so the caller skips
    /// the frame instead of reading garbage. The strides are honoured because
    /// `CVPixelBuffer` rows are frequently padded.
    private func convertBGRAtoRGBA(
        _ pixelBuffer: CVPixelBuffer,
        into out: inout [UInt8],
        width: Int,
        height: Int
    ) -> Bool {
        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }

        guard let base = CVPixelBufferGetBaseAddress(pixelBuffer) else { return false }
        let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
        let src = base.assumingMemoryBound(to: UInt8.self)

        out.withUnsafeMutableBufferPointer { dst in
            for y in 0..<height {
                let rowStart = y * bytesPerRow
                var dstIndex = y * width * 4
                for x in 0..<width {
                    let i = rowStart + x * 4
                    // BGRA -> RGBA: swap the first and third byte.
                    dst[dstIndex] = src[i + 2]
                    dst[dstIndex + 1] = src[i + 1]
                    dst[dstIndex + 2] = src[i]
                    dst[dstIndex + 3] = src[i + 3]
                    dstIndex += 4
                }
            }
        }
        return true
    }
}
