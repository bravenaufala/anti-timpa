import { useCallback, useEffect, useRef, useState } from "react";
import { cameraPreview, isTauri } from "../api";
import type { PreviewFrame } from "../types";

interface CameraPreviewProps {
  /** Whether the preview loop should be running. */
  active: boolean;
  /** Preview width in px. Lower = smaller payload per frame. */
  maxWidth?: number;
  /** Target frames per second. */
  fps?: number;
}

/**
 * Live camera preview.
 *
 * How the frames get here
 * ----------------------
 * There is no `<video>` element and no MediaStream. Frames are captured in
 * Rust, downscaled, JPEG-encoded, and handed over IPC as data URLs which are
 * assigned to `<img src>`. That is a deliberate trade-off: a real MediaStream
 * would be smoother, but it would require the platform camera to be owned by
 * the webview, which is exactly the coupling the rewrite is removing. Here the
 * camera stays owned by Rust and the UI only receives pixels.
 *
 * Two rules keep this from misbehaving
 * -----------------------------------
 * 1. **No overlapping requests.** The next frame is only requested after the
 *    previous one resolves. Without this, a slow frame causes a backlog of
 *    queued `invoke` calls that grows without bound and starves the analysis
 *    capture.
 * 2. **No updates after unmount.** StrictMode unmounts once in development; a
 *    loop that keeps running would keep the camera busy and leak.
 */
export function CameraPreview({ active, maxWidth = 480, fps = 8 }: CameraPreviewProps) {
  const [frame, setFrame] = useState<PreviewFrame | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [stats, setStats] = useState({ count: 0, lastBytes: 0, attempts: 0 });

  /** Guards against state updates after unmount. */
  const aliveRef = useRef(true);
  /** Prevents overlapping IPC calls. */
  const inFlightRef = useRef(false);
  /** Avoids flooding the console with the same error every tick. */
  const lastLoggedErrorRef = useRef<string | null>(null);

  const grabFrame = useCallback(async () => {
    if (inFlightRef.current || !aliveRef.current) return;
    inFlightRef.current = true;
    try {
      const next = await cameraPreview(maxWidth);
      if (!aliveRef.current) return;
      setFrame(next);
      setError(null);
      setStats((s) => ({
        count: s.count + 1,
        lastBytes: next.byte_len,
        attempts: s.attempts + 1,
      }));
    } catch (e) {
      if (!aliveRef.current) return;
      // Preview failures are expected while the device is busy or the session
      // is still starting. Show the message but keep the loop alive so it
      // recovers on its own.
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      setStats((s) => ({ ...s, attempts: s.attempts + 1 }));
      // Log every distinct failure once. Without this a preview that never
      // starts gives no clue why — the UI just sits on the placeholder.
      if (lastLoggedErrorRef.current !== message) {
        lastLoggedErrorRef.current = message;
        console.error("[preview] gagal mengambil frame:", message);
      }
    } finally {
      inFlightRef.current = false;
    }
  }, [maxWidth]);

  useEffect(() => {
    aliveRef.current = true;

    if (!active || !isTauri()) {
      return () => {
        aliveRef.current = false;
      };
    }

    void grabFrame();

    // `setInterval` plus the in-flight guard gives a steady rate without a
    // self-scheduling loop that could drift or stack.
    const intervalMs = Math.max(1000 / Math.max(fps, 1), 60);
    const timer = window.setInterval(() => {
      void grabFrame();
    }, intervalMs);

    return () => {
      aliveRef.current = false;
      window.clearInterval(timer);
    };
  }, [active, fps, grabFrame]);

  if (!isTauri()) {
    return (
      <div className="preview preview-placeholder">
        <span>Pratinjau hanya tersedia di aplikasi Tauri</span>
      </div>
    );
  }

  return (
    <div className="preview">
      <div
        className="preview-stage"
        // Reserve the correct box before the image loads. Without this the
        // layout would jump on every frame as dimensions arrive.
        style={frame ? { aspectRatio: `${frame.width} / ${frame.height}` } : undefined}
      >
        {frame ? (
          <img
            className="preview-img"
            src={frame.data_url}
            alt="Pratinjau kamera"
            draggable={false}
            onError={() => {
              // A CSP block or malformed payload shows up here rather than as
              // a rejected promise, so it needs its own signal.
              setError("Gagal menampilkan gambar pratinjau (kemungkinan diblokir CSP)");
              setFrame(null);
            }}
          />
        ) : (
          <div className="preview-placeholder">
            <span>{error ? "Kamera belum siap" : "Menunggu frame kamera…"}</span>
          </div>
        )}
      </div>

      {error && <div className="preview-error">{error}</div>}

      <div className="preview-stats">
        {frame
          ? `${frame.width}×${frame.height} · ${(stats.lastBytes / 1024).toFixed(0)} KB/frame · ${stats.count} frame`
          : `0 frame · ${stats.attempts} percobaan`}
      </div>
    </div>
  );
}
