package org.antitimpa.antitimpa

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.util.Log
import android.util.Size
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import java.nio.ByteBuffer
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * Owns the CameraX capture session and pumps frames into the Rust core.
 *
 * Usage from the app:
 * ```
 * val bridge = CameraBridge(context, lifecycleOwner)
 * if (bridge.start()) { /* frames now flow */ }
 * // on shutdown
 * bridge.stop()
 * ```
 *
 * The Rust side needs no knowledge of any of this beyond the frame slot; the
 * platform plumbing is contained here.
 */
class CameraBridge(
    private val context: Context,
    private val lifecycleOwner: LifecycleOwner,
) {
    companion object {
        private const val TAG = "ANTITIMPA"

        init {
            // Load the Rust core. Done in a static initializer so a missing
            // library surfaces as a clear error at first use rather than an
            // UnsatisfiedLinkError deep inside a frame callback.
            System.loadLibrary("anti_timpa_lib")
        }

        /**
         * Pushes one RGBA frame into the Rust slot. Implemented in Rust.
         *
         * The buffer must be a direct `ByteBuffer`. The Rust side obtains
         * its address via `GetDirectBufferAddress`, so a heap buffer would be
         * rejected, and passing one as a raw pointer is what caused an earlier
         * `SIGSEGV` inside `memcpy`.
         *
         * The length is read on the Rust side from the buffer's capacity, so it
         * cannot disagree with the actual allocation.
         */
        @JvmStatic
        private external fun antitimpa_push_frame(
            width: Int,
            height: Int,
            rotationDegrees: Int,
            buffer: ByteBuffer,
        ): Boolean

        /** Marks the native stream active/inactive. Implemented in Rust. */
        @JvmStatic
        private external fun antitimpa_set_stream_active(active: Boolean)

        /** Frames delivered so far. Implemented in Rust. */
        @JvmStatic
        private external fun antitimpa_frames_received(): Long
    }

    private var provider: ProcessCameraProvider? = null
    private var executor: ExecutorService? = null

    /** Frames handed to Rust since [start]. Mirrors the Rust-side counter. */
    val framesPushed: Long
        get() = antitimpa_frames_received()

    /**
     * Returns `true` when the CAMERA permission has been granted.
     *
     * Callers should request the permission themselves before calling [start],
     * so the prompt is tied to a user action in the UI.
     */
    fun hasPermission(): Boolean =
        ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED

    /**
     * Starts the capture session.
     *
     * Returns `false` (with a logcat explanation) when the permission is
     * missing or the camera cannot be bound. It returns a status instead of
     * throwing so the caller can show a UI message and fall back without a
     * crash handler.
     */
    fun start(): Boolean {
        if (!hasPermission()) {
            Log.e(TAG, "[camera] izin CAMERA belum diberikan; bridge tidak dimulai")
            return false
        }

        val exec = Executors.newSingleThreadExecutor()
        executor = exec

        val future = ProcessCameraProvider.getInstance(context)
        future.addListener({
            try {
                val cameraProvider = future.get()
                provider = cameraProvider

                // Analyse at a bounded resolution: the analysis is one-shot and
                // does a QR decode, so a huge buffer would only cost time and
                // memory without improving decode success.
                val resolutionSelector = ResolutionSelector.Builder()
                    .setResolutionStrategy(
                        ResolutionStrategy(
                            Size(1280, 720),
                            ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER_THEN_LOWER,
                        ),
                    )
                    .build()

                val analysis = ImageAnalysis.Builder()
                    .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .setOutputImageFormat(ImageAnalysis.OUTPUT_IMAGE_FORMAT_RGBA_8888)
                    .setResolutionSelector(resolutionSelector)
                    .build()

                // Lambda parameter types are spelled out: CameraFrameAnalyzer's
                // constructor takes a function type, and Kotlin cannot infer
                // the parameter types from the constructor call alone.
                val analyzer = CameraFrameAnalyzer(
                    object : CameraFrameAnalyzer.FrameSink {
                        override fun onFrame(
                            width: Int,
                            height: Int,
                            rotation: Int,
                            rgba: ByteBuffer,
                        ): Boolean = antitimpa_push_frame(width, height, rotation, rgba)
                    },
                )

                analysis.setAnalyzer(exec, analyzer)

                cameraProvider.unbindAll()
                cameraProvider.bindToLifecycle(
                    lifecycleOwner,
                    CameraSelector.DEFAULT_BACK_CAMERA,
                    analysis,
                )

                antitimpa_set_stream_active(true)
                Log.i(TAG, "[camera] sesi CameraX aktif (RGBA_8888, back camera)")
                return@addListener
            } catch (t: Throwable) {
                // Covers SecurityException (permission revoked mid-flight),
                // IllegalStateException (no camera), and binding failures.
                Log.e(TAG, "[camera] gagal memulai CameraX: ${t.message}", t)
                antitimpa_set_stream_active(false)
                stop()
            }
        }, ContextCompat.getMainExecutor(context))

        return true
    }

    /**
     * Stops the session and releases the camera.
     *
     * Safe to call repeatedly, matching the Rust side's idempotent `release`:
     * React StrictMode can trigger teardown more than once.
     */
    fun stop() {
        try {
            provider?.unbindAll()
        } catch (t: Throwable) {
            Log.w(TAG, "[camera] unbindAll gagal saat stop: ${t.message}")
        }
        provider = null

        executor?.shutdown()
        executor = null

        antitimpa_set_stream_active(false)
        Log.i(TAG, "[camera] sesi dihentikan; total frame dikirim=${antitimpa_frames_received()}")
    }
}
