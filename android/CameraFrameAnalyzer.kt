package org.antitimpa.antitimpa

import android.util.Log
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import java.nio.ByteBuffer

/**
 * CameraX analyzer that forwards frames to the Rust core.
 *
 * This is the Kotlin half of the mobile camera bridge; the Rust half lives in
 * `src-tauri/src/camera/mobile/android.rs`. Together they replace the
 * Camera4Kivy + `analyze_pixels_callback` path the old Kivy app used, which
 * required the analysis to read pixels back out of a UI widget.
 *
 * Design notes
 * ------------
 * * **One-shot semantics.** Frames are pushed continuously but the Rust slot
 *   keeps only the newest one. A shutter press then drains it. This matches
 *   the old app's hard-won finding that decoding every frame in a stream
 *   causes hangs; preview stays cheap and analysis happens on demand.
 * * **RGBA, not YUV.** CameraX hands out `YUV_420_888` by default, but the
 *   Rust side expects RGBA8888. The conversion happens here because Kotlin has
 *   the JVM's colour-conversion intrinsics; doing it in Rust would mean
 *   reimplementing YUV->RGB by hand. Sending YUV directly would produce a
 *   wrong-sized buffer, which the Rust side rejects rather than corrupting
 *   memory.
 * * **Direct buffers.** The converted RGBA goes into a direct `ByteBuffer` so
 *   the native read needs no extra copy through the JVM heap.
 * * **Failures are logged, not thrown.** This runs on the camera's executor
 *   for every frame; an exception here would kill the analysis pipeline.
 */
class CameraFrameAnalyzer(
    private val sink: FrameSink,
) : ImageAnalysis.Analyzer {

    /**
     * Callback for a ready RGBA frame.
     *
     * Declared as a named interface rather than a bare function type: Kotlin
     * cannot infer parameter types for a lambda passed to a constructor, and
     * spelling them out inline is noisy and easy to get wrong. A named
     * interface makes the contract explicit and self-documenting.
     */
    interface FrameSink {
        /**
         * @return `true` when the frame was accepted. `false` means the native
         *   side rejected it (bad dimensions or an undersized buffer), which
         *   the caller should log rather than ignore.
         */
        fun onFrame(width: Int, height: Int, rotation: Int, rgba: ByteBuffer): Boolean
    }

    companion object {
        private const val TAG = "ANTITIMPA"
    }

    /** Reused across frames to avoid allocating a buffer per frame. */
    private var reusedBuffer: ByteBuffer? = null

    override fun analyze(image: ImageProxy) {
        try {
            val width = image.width
            val height = image.height
            val rotation = image.imageInfo.rotationDegrees

            val rgba = convertToRgba(image)
            if (rgba == null) {
                Log.w(TAG, "[camera] konversi RGBA gagal; frame dilewati")
                return
            }

            val accepted = sink.onFrame(width, height, rotation, rgba)
            if (!accepted) {
                // The Rust side rejects bad dimensions or undersized buffers.
                // Log so a systemic problem is visible in logcat without
                // flooding it.
                Log.w(TAG, "[camera] native menolak frame ${width}x${height} (rotasi=$rotation)")
            }
        } catch (t: Throwable) {
            // Never let an analyzer exception kill the capture pipeline.
            Log.e(TAG, "[camera] error saat menganalisis frame: ${t.message}", t)
        } finally {
            // CameraX will not deliver another frame until this is called.
            // Closing in `finally` guarantees progress even on error.
            image.close()
        }
    }

    /**
     * Converts an `ImageProxy` plane to a packed RGBA8888 direct buffer.
     *
     * Returns `null` when the plane layout is unexpected, so the caller can
     * skip the frame instead of reading garbage.
     */
    private fun convertToRgba(image: ImageProxy): ByteBuffer? {
        val plane = image.planes.firstOrNull() ?: return null
        val buffer = plane.buffer
        val rowStride = plane.rowStride
        val pixelStride = plane.pixelStride

        val width = image.width
        val height = image.height
        if (width <= 0 || height <= 0) {
            Log.w(TAG, "[camera] dimensi ImageProxy tidak valid: ${width}x${height}")
            return null
        }
        val needed = width * height * 4

        // Reuse the scratch buffer when possible; allocating ~1.5 MB per frame
        // at 30 fps would create real GC pressure.
        //
        // Structured so the working variable is non-null: Kotlin cannot
        // smart-cast a nullable `var` across the assignment below, which would
        // force `!!` on every use.
        val out: ByteBuffer = reusedBuffer?.takeIf { it.capacity() >= needed }
            ?: ByteBuffer.allocateDirect(needed).also { reusedBuffer = it }

        out.clear()

        // Fast path: tightly packed RGBA already.
        if (pixelStride == 4 && rowStride == width * 4) {
            out.put(buffer)
            out.rewind()
            return out
        }

        // General path: walk rows honouring the strides, which is what makes
        // this correct for the padding CameraX adds to plane rows.
        val row = ByteArray(width * pixelStride)
        for (y in 0 until height) {
            val rowStart = y * rowStride
            if (rowStart + row.size > buffer.limit()) break
            buffer.position(rowStart)
            buffer.get(row)

            var src = 0
            for (x in 0 until width) {
                // Plane order for RGBA_8888 is R,G,B,A.
                out.put(row[src])
                out.put(row[src + 1])
                out.put(row[src + 2])
                out.put(row[src + 3])
                src += pixelStride
            }
        }

        out.rewind()
        return out
    }
}
