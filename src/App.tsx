import { useCallback, useEffect, useRef, useState } from "react";
import {
  analyzePayload,
  cameraDiagnostics,
  captureAndAnalyze,
  isTauri,
  releaseCamera,
} from "./api";
import type { CameraDiagnostics, OpticalType, ScanSnapshot } from "./types";
import { RiskGauge } from "./components/RiskGauge";
import { DetailPanel } from "./components/DetailPanel";
import { CameraPanel } from "./components/CameraPanel";
import { CameraPreview } from "./components/CameraPreview";
import { SAMPLE_PAYLOADS } from "./samples";

const EMPTY_SNAPSHOT: ScanSnapshot = {
  l1: {
    l1_score: 0,
    spatial_edge_density: 0,
    temporal_glare_var: 0,
    risk_level: "NO QR",
  },
  l2: {
    l2_score: 0,
    crc_valid: true,
    initiation_mode: "",
    mcc: "",
    merchant_name: "",
    merchant_city: "",
    parsed_tlv: {},
    warnings: [],
  },
  l3: {
    l3_score: 0,
    risk_level: "NO QR",
    warnings: [],
    client_city: null,
    merchant_city: null,
  },
  combined_score: 0,
  combined_risk_level: "NO QR",
  is_blurry: false,
  blur_var: 0,
  qr_bbox: null,
  raw_qris_str: "",
  no_qr_reason: null,
};

export default function App() {
  const [snapshot, setSnapshot] = useState<ScanSnapshot>(EMPTY_SNAPSHOT);
  const [payload, setPayload] = useState("");
  const [clientCity, setClientCity] = useState("");
  const [opticalType, setOpticalType] = useState<OpticalType>("physical_camera_scan");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [camera, setCamera] = useState<CameraDiagnostics | null>(null);
  /**
   * Preview is opt-in and off by default.
   *
   * Each preview frame costs a capture plus a JPEG encode in Rust, so leaving
   * it running permanently would keep the camera device busy and burn battery
   * for no benefit when the user is not actively framing a QR.
   */
  const [previewOn, setPreviewOn] = useState(false);
  /**
   * Tracks whether this is the real unmount rather than React StrictMode's
   * intentional throwaway unmount.
   *
   * In development StrictMode mounts, unmounts, and remounts every component
   * once. The cleanup of that throwaway unmount used to call `releaseCamera`,
   * which killed the camera before the user's first capture — the
   * "kamera desktop sudah dilepas" bug. The Rust side now self-heals on
   * capture, but the correct fix is also to not release on a spurious unmount.
   */
  const releasedRef = useRef(false);

  const refreshCamera = useCallback(async () => {
    if (!isTauri()) return;
    try {
      setCamera(await cameraDiagnostics());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  // Probe the camera on mount so the user learns immediately whether they are
  // on real hardware or the synthetic fallback.
  useEffect(() => {
    void refreshCamera();
  }, [refreshCamera]);

  // Release the device when the window really goes away, mirroring the old
  // app's `on_stop` hook.
  useEffect(() => {
    if (!isTauri()) return;

    // Re-arm on every mount. StrictMode's remount flips this back, so a later
    // genuine teardown still releases exactly once.
    releasedRef.current = false;

    const release = () => {
      if (releasedRef.current) return;
      releasedRef.current = true;
      void releaseCamera().catch(() => undefined);
    };

    window.addEventListener("beforeunload", release);

    // Deliberately NOT calling release() here.
    //
    // Effect cleanup is not "the app is closing" — React StrictMode runs it on
    // an intentional throwaway unmount in development, and the window may also
    // be hidden and reshown. Releasing there killed the camera before the
    // user's first capture. `beforeunload` covers real teardown, and Rust's
    // `Drop` handles process exit, so no unmount-time release is needed.
    return () => {
      window.removeEventListener("beforeunload", release);
    };
  }, []);

  const runCapture = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await captureAndAnalyze(opticalType, clientCity.trim() || null);
      setSnapshot(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      // Refresh diagnostics even on failure: the counters and last error are
      // exactly what is needed to explain why it failed.
      await refreshCamera();
      setBusy(false);
    }
  }, [opticalType, clientCity, refreshCamera]);

  const runAnalysis = async (raw: string) => {
    const trimmed = raw.trim();
    if (!trimmed) {
      setError("Payload QRIS masih kosong.");
      return;
    }

    setBusy(true);
    setError(null);
    try {
      const result = await analyzePayload(trimmed, opticalType, clientCity.trim() || null);
      setSnapshot(result);
      setPayload(trimmed);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const hasResult = snapshot.combined_risk_level !== "NO QR";

  return (
    <div className="app">
      <header className="topbar">
        <h1>Anti Timpa QRIS Scanner</h1>
        <span className="topbar-sub">
          React + Tauri · Layer 2 &amp; 3 aktif{isTauri() ? "" : " · mode browser (tanpa backend)"}
        </span>
      </header>

      {!isTauri() && (
        <div className="notice">
          Buka lewat <code>npm run tauri:dev</code> agar panggilan ke Rust core aktif.
        </div>
      )}

      <main className="content">
        <section className="panel">
          <RiskGauge score={snapshot.combined_score} level={snapshot.combined_risk_level} />
          {snapshot.no_qr_reason && (
            <p className="hint hint-action">{snapshot.no_qr_reason}</p>
          )}
          {!hasResult && !snapshot.no_qr_reason && (
            <p className="hint">
              Ambil foto QRIS lewat kamera, atau tempel payload di bawah.
            </p>
          )}
        </section>

        <section className="panel">
          <label className="toggle">
            <input
              type="checkbox"
              checked={previewOn}
              onChange={(e) => setPreviewOn(e.target.checked)}
              disabled={!isTauri()}
            />
            <span>Pratinjau kamera langsung</span>
          </label>

          {/*
           * Paused while an analysis capture is in flight.
           *
           * Preview and analysis share one camera device, and the driver
           * returns "already taken" rather than queueing when both ask at once.
           * Without this pause, a preview tick landing mid-capture would make
           * the user's scan fail intermittently — the worst kind of bug to
           * diagnose. Pausing costs one preview frame and removes the race.
           */}
          {previewOn && <CameraPreview active={previewOn && !busy} />}
        </section>

        <CameraPanel
          info={camera}
          busy={busy}
          onCapture={runCapture}
          disabled={!isTauri()}
        />

        <section className="panel">
          <span className="row-label">Analisis Manual (tanpa kamera)</span>
          <label className="field">
            <span className="row-label">Payload QRIS</span>
            <textarea
              className="input"
              rows={4}
              value={payload}
              placeholder="00020101021126..."
              onChange={(e) => setPayload(e.target.value)}
            />
          </label>

          <label className="field">
            <span className="row-label">Kota Klien (opsional)</span>
            <input
              className="input"
              type="text"
              value={clientCity}
              placeholder="mis. Bandung"
              onChange={(e) => setClientCity(e.target.value)}
            />
          </label>

          <label className="field">
            <span className="row-label">Konteks Optik</span>
            <select
              className="input"
              value={opticalType}
              onChange={(e) => setOpticalType(e.target.value as OpticalType)}
            >
              <option value="physical_camera_scan">Kamera fisik (physical_camera_scan)</option>
              <option value="imported_image">Gambar impor (imported_image)</option>
            </select>
          </label>

          <button className="primary" disabled={busy} onClick={() => runAnalysis(payload)}>
            {busy ? "Menganalisis…" : "Analisis"}
          </button>

          {error && <div className="error">{error}</div>}
        </section>

        <section className="panel">
          <span className="row-label">Contoh Payload</span>
          <div className="samples">
            {SAMPLE_PAYLOADS.map((s) => (
              <button
                key={s.label}
                className="sample"
                disabled={busy}
                title={s.description}
                onClick={() => runAnalysis(s.payload)}
              >
                {s.label}
              </button>
            ))}
          </div>
        </section>

        {hasResult && (
          <DetailPanel
            l1={snapshot.l1}
            l2={snapshot.l2}
            l3={snapshot.l3}
            rawPayload={snapshot.raw_qris_str}
            blurVar={snapshot.blur_var}
            isBlurry={snapshot.is_blurry}
          />
        )}
      </main>
    </div>
  );
}
