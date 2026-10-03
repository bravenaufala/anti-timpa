import { useCallback, useEffect, useRef, useState } from "react";
import {
  analyzePayload,
  cameraDiagnostics,
  captureAndAnalyze,
  isTauri,
  recordScan,
  releaseCamera,
} from "./api";
import type { CameraDiagnostics, OpticalType, ScanSnapshot } from "./types";
import { describeLocation, resolveLocation, type ResolvedLocation } from "./location";
import { RiskGauge } from "./components/RiskGauge";
import { DetailPanel } from "./components/DetailPanel";
import { CoverageBanner } from "./components/CoverageBanner";
import { FindingsList } from "./components/FindingsList";
import { ScanResultCard } from "./components/ScanResultCard";
import { HistoryPanel } from "./components/HistoryPanel";
import { CameraPanel } from "./components/CameraPanel";
import { CameraPreview } from "./components/CameraPreview";
import { ImageImportPanel } from "./components/ImageImportPanel";
import { SAMPLE_PAYLOADS } from "./samples";

const EMPTY_SNAPSHOT: ScanSnapshot = {
  l1: {
    l1_score: 0,
    spatial_edge_density: 0,
    temporal_glare_var: 0,
    texture_discontinuity: 0,
    glare_fraction: 0,
    risk_level: "NO QR",
    quiet_zone_truncated: false,
    warnings: [],
  },
  l2: {
    l2_score: 0,
    crc_valid: true,
    initiation_mode: "",
    mcc: "",
    merchant_name: "",
    merchant_city: "",
    merchant_id: "",
    parsed_tlv: {},
    warnings: [],
  },
  l3: {
    l3_score: 0,
    risk_level: "NO QR",
    warnings: [],
    client_city: null,
    merchant_city: null,
    mismatch_kind: "NOT_EVALUATED",
    distance_km: null,
    location_available: false,
    evaluated: false,
  },
  combined_score: 0,
  combined_risk_level: "NO QR",
  is_blurry: false,
  blur_var: 0,
  qr_bbox: null,
  raw_qris_str: "",
  no_qr_reason: null,
  scannable: false,
  error_reason: null,
  coverage: {
    optical_ran: false,
    payload_ran: false,
    geofence_ran: false,
    complete: false,
    summary: "Belum ada pemindaian.",
  },
  findings: [],
  chain_hash: null,
};

/** Remembers the UI mode between launches. */
const TECHNICAL_KEY = "antitimpa.technical";

/**
 * Attempts a coarse device position.
 *
 * The logic lives in `./location`. `navigator.geolocation` is deliberately not
 * called directly: it does not work inside the Tauri WebView on desktop, and a
 * swallowed error made Layer 3 report "location unavailable" on every scan
 * while the UI still showed a location checkbox.
 */

export default function App() {
  const [snapshot, setSnapshot] = useState<ScanSnapshot>(EMPTY_SNAPSHOT);
  const [payload, setPayload] = useState("");
  const [clientCity, setClientCity] = useState("");
  const [opticalType, setOpticalType] = useState<OpticalType>("physical_camera_scan");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [camera, setCamera] = useState<CameraDiagnostics | null>(null);
  const [useDeviceGps, setUseDeviceGps] = useState(true);
  const [lastLocation, setLastLocation] = useState<ResolvedLocation | null>(null);
  const [historyRevision, setHistoryRevision] = useState(0);
  /**
   * Drives the whole UI: off is a simple, plain-language view for a shopper;
   * on exposes the layer scores, raw payload, camera diagnostics, and the other
   * tools used while developing and calibrating.
   */
  const [technical, setTechnical] = useState<boolean>(() => {
    if (typeof window === "undefined") return false;
    return window.localStorage.getItem(TECHNICAL_KEY) === "1";
  });
  /**
   * Preview is on by default so the camera is already framing a QR the moment
   * the app opens; the toggle still lets the user switch it off.
   *
   * Each preview frame costs a capture plus a JPEG encode in Rust, so an idle
   * preview keeps the camera device busy and burns battery. Defaulting it on is
   * a deliberate trade for a camera-first app where framing is the first step.
   */
  const [previewOn, setPreviewOn] = useState(true);
  /**
   * Tracks whether this is the real unmount rather than React StrictMode's
   * intentional throwaway unmount.
   *
   * In development StrictMode mounts, unmounts, and remounts every component
   * once. The cleanup of that throwaway unmount used to call `releaseCamera`,
   * which killed the camera before the user's first capture (the
   * "kamera desktop sudah dilepas" bug). The Rust side now self-heals on
   * capture, but the fix is also to not release on a spurious unmount.
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

  // Release the device when the window is actually closing.
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

    // release() is not called here on purpose.
    //
    // Effect cleanup does not mean "the app is closing". React StrictMode runs
    // it on an intentional throwaway unmount in development, and the window may
    // also be hidden and reshown. Releasing there killed the camera before the
    // user's first capture. `beforeunload` covers real teardown, and Rust's
    // `Drop` handles process exit, so no unmount-time release is needed.
    return () => {
      window.removeEventListener("beforeunload", release);
    };
  }, []);

  const toggleTechnical = (on: boolean) => {
    setTechnical(on);
    try {
      window.localStorage.setItem(TECHNICAL_KEY, on ? "1" : "0");
    } catch {
      // A blocked localStorage (private mode) must not break the toggle.
    }
  };

  /**
   * Builds the location argument for Layer 3.
   *
   * Always attempts a resolution, whether or not the user enabled device GPS:
   * the typed city name alone is enough to reach the offline table, and refusing
   * to try meant Layer 3 reported "not run" even when the user had typed a city.
   * The device-GPS toggle only controls whether a *precise* fix is requested on
   * top of that.
   */
  const buildLocation = useCallback(async (): Promise<ResolvedLocation> => {
    const resolved = await resolveLocation(clientCity, useDeviceGps);
    setLastLocation(resolved);
    return resolved;
  }, [clientCity, useDeviceGps]);

  /**
   * Publishes a scan result and saves it to the local history.
   *
   * Saving is automatic and unconditional for scannable results, so the user
   * does not have to remember to keep a scan. Results without a readable QR are
   * rejected by the backend and never reach the history, so a failed read
   * cannot create a row.
   */
  const commitResult = useCallback(async (result: ScanSnapshot, source: string) => {
    setSnapshot(result);

    if (!isTauri() || !result.scannable) return;
    try {
      await recordScan(result, source, Date.now());
      setHistoryRevision((n) => n + 1);
    } catch {
      // A failed save must not hide the result the user just scanned; the
      // history panel will simply not show the new row.
    }
  }, []);

  const runCapture = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const location = await buildLocation();
      const result = await captureAndAnalyze(opticalType, location);
      await commitResult(result, "camera");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      // Refresh diagnostics even on failure: the counters and last error are
      // exactly what is needed to explain why it failed.
      await refreshCamera();
      setBusy(false);
    }
  }, [opticalType, buildLocation, commitResult, refreshCamera]);

  const runAnalysis = async (raw: string, source = "manual") => {
    const trimmed = raw.trim();
    if (!trimmed) {
      setError("Payload QRIS masih kosong.");
      return;
    }

    setBusy(true);
    setError(null);
    try {
      const location = await buildLocation();
      const result = await analyzePayload(trimmed, opticalType, location);
      setPayload(trimmed);
      await commitResult(result, source);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  /** Applied by the image import panel in technical mode. */
  const applyResult = (result: ScanSnapshot, source: string) => {
    void commitResult(result, source);
  };

  const hasResult = snapshot.scannable;

  return (
    <div className="app">
      <header className="topbar">
        <div className="topbar-main">
          <h1>Anti Timpa QRIS Scanner</h1>
          <label className="switch" title="Tampilkan detail teknis">
            <input
              type="checkbox"
              checked={technical}
              onChange={(e) => toggleTechnical(e.target.checked)}
            />
            <span>Mode teknis</span>
          </label>
        </div>
        <span className="topbar-sub">
          {technical
            ? `React + Tauri · Layer 1, 2 & 3 aktif${isTauri() ? "" : " · mode browser (tanpa backend)"}`
            : "Periksa keamanan kode QRIS sebelum membayar"}
        </span>
      </header>

      {!isTauri() && (
        <div className="notice">
          Buka lewat <code>npm run tauri:dev</code> agar panggilan ke Rust core aktif.
        </div>
      )}

      <main className="content">
        {hasResult && technical ? (
          <section className="panel">
            <RiskGauge score={snapshot.combined_score} level={snapshot.combined_risk_level} />
            {snapshot.coverage.summary && <CoverageBanner coverage={snapshot.coverage} />}
            <FindingsList findings={snapshot.findings} technical />
          </section>
        ) : (
          <ScanResultCard snapshot={snapshot} busy={busy} />
        )}

        <section className="panel">
          <label className="toggle">
            <input
              type="checkbox"
              checked={previewOn}
              onChange={(e) => setPreviewOn(e.target.checked)}
              disabled={!isTauri()}
            />
            <span>Pratinjau kamera</span>
          </label>

          {/*
           * Paused while an analysis capture is in flight.
           *
           * Preview and analysis share one camera device, and the driver
           * returns "already taken" rather than queueing when both ask at once.
           * Without this pause, a preview tick landing mid-capture would make
           * the user's scan fail intermittently. Pausing costs one preview frame
           * and removes the race.
           */}
          {previewOn && <CameraPreview active={previewOn && !busy} technical={technical} />}
        </section>

        {/*
         * The scan button lives below the preview so the framing the user sees
         * sits directly above the control that captures it.
         */}
        <CameraPanel
          info={camera}
          busy={busy}
          onCapture={runCapture}
          disabled={!isTauri()}
          technical={technical}
        />

        {technical && (
          <>
            <ImageImportPanel
              busy={busy}
              opticalType={opticalType}
              location={{
                city: clientCity.trim() || null,
                lat: lastLocation?.lat ?? null,
                lon: lastLocation?.lon ?? null,
              }}
              onResult={applyResult}
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

              <label className="toggle">
                <input
                  type="checkbox"
                  checked={useDeviceGps}
                  onChange={(e) => setUseDeviceGps(e.target.checked)}
                />
                <span>Sertakan lokasi perangkat (untuk estimasi jarak)</span>
              </label>
              <p className="hint hint-small">
                Di ponsel, ini meminta izin lokasi ke sistem operasi. Di desktop tidak
                ada layanan lokasi sistem, jadi koordinat diambil dari tabel kota
                offline bawaan berdasarkan nama kota di atas. Koordinat hanya dipakai
                untuk menghitung jarak ke kota merchant, dan tidak dikirim ke mana pun.
              </p>
              {lastLocation && (
                <p className="hint hint-small">
                  Lokasi terakhir dipakai: <strong>{describeLocation(lastLocation)}</strong>
                  {lastLocation.note ? ` — ${lastLocation.note}` : ""}
                </p>
              )}

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

              {/*
               * Analisis manual cannot run Layer 1: a pasted payload carries no
               * pixels. Saying so here, before the button is pressed, is cheaper
               * than letting the user infer it from a coverage warning afterwards.
               */}
              <p className="hint hint-small">
                Catatan: analisis manual hanya menjalankan Layer 2 dan 3. Penempelan
                fisik pada QR tidak dapat dideteksi tanpa gambar — gunakan kamera
                untuk itu.
              </p>

              <button className="primary" disabled={busy} onClick={() => void runAnalysis(payload)}>
                {busy ? "Menganalisis…" : "Analisis"}
              </button>
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
                    onClick={() => void runAnalysis(s.payload, "sample")}
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
          </>
        )}

        {error && (
          <div className="error">
            {technical
              ? error
              : "Terjadi masalah saat memindai. Coba lagi; aktifkan mode teknis untuk melihat detail."}
          </div>
        )}

        <HistoryPanel
          revision={historyRevision}
          disabled={!isTauri()}
          technical={technical}
        />
      </main>
    </div>
  );
}
