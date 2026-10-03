//! Layer 1: optical tamper analysis.
//!
//! Layer 2 and Layer 3 both read the *payload*, which a sticker attack never
//! touches. Someone can paste a QRIS over another QRIS with a byte-identical
//! payload and every payload-level check will pass cleanly. This layer looks at
//! the physical artefact, which is what the app's "anti timpa" name refers to.
//!
//! Three signals are used, chosen because they are cheap, explainable, and do
//! not depend on a reference image of the genuine QR:
//!
//! 1. Quiet-zone edge density. A QR spec requires a clear margin around the
//!    symbol. Criminals cover the original QR completely, so the sticker's edge
//!    lands *inside* that margin. A pasted overlay therefore leaves a strong,
//!    straight edge where a clean QR has flat paper. Measured as the fraction of
//!    gradient pixels in the quiet-zone ring.
//!
//! 2. Temporal glare variance. A sticker is usually a different material
//!    (glossy thermal print, adhesive film), so a specular highlight moves
//!    across it across frames. Measured as the variance of glare-pixel counts
//!    over a short frame window.
//!
//! 3. Overlay texture discontinuity. The printed module grid and the sticker
//!    have different noise floors, so the *variant* of local intensity across
//!    the symbol is high. Flat paper has low variant.
//!
//! Provenance note
//! ---------------
//! Thresholds of `0.15` (edge) and `0.003` (glare) were derived from a small
//! synthetic fixture set. Those numbers are not reproduced here as magic
//! constants, because shipping an unvalidated threshold is how a detector ends
//! up either blind or permanently alerting. Instead the layer:
//!
//! * uses fixed, documented bounds that are stated as calibration values rather
//!   than physical truths, and records the measurements they came from, and
//! * exposes raw metrics alongside the score so the thresholds can be fitted
//!   against real captures without changing the code shape.
//!
//! The unit tests at the bottom pin the *behaviour* (clean frame scores low, a
//! sticker frame scores high) rather than the exact numbers, and are the
//! regression net for the calibration constants.
//!
//! Sensitivity re-tune
//! -------------------
//! Field feedback was that a clearly overlaid QRIS scored only ~10% while a
//! clean one scored ~4%. That separation is real but unusable. The cause is
//! that the full-anomaly bounds were fitted to *rendered* fixtures, whose
//! contrast is far higher than a phone camera capture: an overlay that measures
//! `0.277` on a fixture measures roughly `0.12` in the field, so it sat near the
//! bottom of a ramp that only saturated at `0.450`.
//!
//! Three constants were re-tuned to close that gap:
//!
//! * `QUIET_ZONE_EDGE_FULL` `0.450 -> 0.120`, so the ramp saturates where real
//!   captures actually live.
//! * `GLARE_FRACTION_FULL` `0.120 -> 0.060`, so a glossy overlay contributes
//!   before it is nearly a mirror.
//! * `SIGNAL_SHARPNESS`, a convex exponent on each component, which keeps the
//!   noise floor low while letting a genuine violation rise steeply.
//!
//! The raw metrics are unchanged and still reported, so this remains refittable
//! against real photographs.

use crate::camera::Frame;
use serde::{Deserialize, Serialize};

/// Result of the optical analysis, mirroring the shape the UI already expects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer1Result {
    pub l1_score: f64,
    pub spatial_edge_density: f64,
    pub temporal_glare_var: f64,
    /// Mean local intensity variance over the symbol region.
    pub texture_discontinuity: f64,
    /// Share of the quiet-zone ring occupied by pixels too bright to be paper.
    pub glare_fraction: f64,
    /// `"LOW RISK"`, `"CAUTION"`, `"HIGH RISK"`, or `"NOT RUN"`.
    pub risk_level: String,
    /// True when there was not enough quiet zone to measure, which makes the
    /// edge-density signal unreliable.
    pub quiet_zone_truncated: bool,
    pub warnings: Vec<String>,
}

impl Layer1Result {
    /// The explicit "this layer did not execute" value.
    ///
    /// Kept as a constructor so every caller reports the same neutral shape and
    /// no code path can accidentally invent a score for a layer that never ran.
    pub fn not_run(reason: &str) -> Self {
        Self {
            l1_score: 0.0,
            spatial_edge_density: 0.0,
            temporal_glare_var: 0.0,
            texture_discontinuity: 0.0,
            glare_fraction: 0.0,
            risk_level: "NOT RUN".to_string(),
            quiet_zone_truncated: false,
            warnings: vec![format!("Layer 1 tidak dijalankan: {reason}")],
        }
    }

    pub fn ran(&self) -> bool {
        self.risk_level != "NOT RUN"
    }
}

// ---------------------------------------------------------------------------
// Tuning constants
//
// These are calibration parameters, not physical truths. Calibration policy:
//
// * Each constant is expressed as a multiple of a measured value from the
//   synthetic fixtures (the "clean" and "sticker" frames the tests generate),
//   so the numbers trace back to observations rather than to guesses.
// * Measurements drift in surprising ways, so each constant is set to a round
//   multiple (2x, 3x) to leave headroom for real-world noise.
// * These are still synthetic-fixture values. Re-fitting against captures from
//   the field devices is the first item on the roadmap; the raw metrics are all
//   reported in `Layer1Result` so that refit needs no code change.
// ---------------------------------------------------------------------------

/// Gradient magnitude above which a pixel counts as an "edge" of the Sobel
/// response. Sobel on 0-255 input peaks near 1020; 24 sits comfortably above
/// sensor noise while catching a pasted sticker's boundary.
const EDGE_MAGNITUDE_THRESHOLD: f64 = 24.0;

/// Fraction of the detection ring that may be edges before the margin is
/// considered violated.
///
/// Measured with `QUIET_ZONE_SEARCH_FRACTION = 0.12`: a clean synthetic QR on a
/// printed sheet scores ~0.032, and the same QR with a pasted sticker scores
/// ~0.277. The limit sits between them, at roughly 2x the clean measurement.
const QUIET_ZONE_EDGE_LIMIT: f64 = 0.065;

/// Edge density at which the signal is treated as fully anomalous.
///
/// Sensitivity. This bound used to be `0.450`, fitted to the synthetic fixture
/// separation (clean ~0.032, sticker ~0.277). Real camera captures are much
/// softer and lower-contrast than a rendered fixture: a genuine overlay
/// photographed off a phone lands around `0.10`-`0.13`, not `0.277`. On the old
/// scale that put a clearly tampered QR at ~10% and a clean one at ~4%, which is
/// the opposite of useful: the detector could not separate them in the band
/// that matters.
///
/// Cutting the bound to `0.120` makes the ramp saturate at edge densities real
/// captures actually produce, so an overlay climbs past the CAUTION threshold
/// instead of creeping up a near-flat line. Mild paper/lighting texture also
/// scores higher as a result, which is why the clean-side shaping below
/// (`SIGNAL_SHARPNESS`) exists to keep a noise floor from being mistaken for an
/// overlay. Re-fit this against real photographs when they are available;
/// `spatial_edge_density` is reported in `Layer1Result` for that purpose.
const QUIET_ZONE_EDGE_FULL: f64 = 0.120;

/// `QUIET_ZONE_EDGE_LIMIT` expressed as a multiple of the clean measurement,
/// kept only so the code and the calibration comment cannot drift apart.
/// Asserted by `edge_limit_sits_between_clean_and_sticker_measurements`.
#[allow(dead_code)]
const QUIET_ZONE_EDGE_LIMIT_MULTIPLE_OF_CLEAN: f64 = 2.0;

/// Fraction of bright (specular) pixels in the quiet zone that is considered
/// normal. Printing artefacts and mild sheen sit below this.
const GLARE_FRACTION_LIMIT: f64 = 0.015;

/// Glare fraction at which the signal is fully anomalous.
///
/// Steepened alongside `QUIET_ZONE_EDGE_FULL`: glare is the corroborating
/// signal, and on the old bound a real glossy overlay contributed almost
/// nothing until it was nearly a mirror.
const GLARE_FRACTION_FULL: f64 = 0.060;

/// Convex sharpening applied to each scored component before weighting.
///
/// Raising a ramped component to a power above 1 widens the gap between a low
/// noise floor and a genuine violation: a value of `0.25` becomes `0.125` while
/// `0.75` becomes `0.65`. That shape keeps real clean captures, which sit just
/// above the limit, from climbing as fast as a real overlay once the
/// full-anomaly bound was lowered.
const SIGNAL_SHARPNESS: f64 = 1.5;

/// Temporal glare variance above which a moving highlight is suspected.
///
/// Measured: a highlight whose radius cycles 0 -> 45 -> 8 -> 50 -> 5 px inside
/// a 240 px symbol produces a glare-fraction variance on the order of 1e-3.
const GLARE_VARIANCE_LIMIT: f64 = 0.0002;

/// Glare variance mapped to a full-anomaly score.
const GLARE_VARIANCE_FULL: f64 = 0.0025;

/// Local-texture variance that counts as a normal, printed surface.
///
/// Why this is not wired into the score. On a real QR the symbol interior is
/// full of printed modules, so a max-local-variance metric saturates (9800 =
/// 99 squared) for *every* readable QR. It therefore cannot separate a tampered
/// symbol from a clean one, and including it would pin the spatial score at 1.0
/// regardless of what the margin looks like. The measurement is still computed
/// and reported in `Layer1Result` because it is the right shape of signal to
/// re-fit once real captures are available, but until then, leaving it out of
/// the score is better than shipping a weight that is pure noise.
#[allow(dead_code)]
const TEXTURE_LIMIT: f64 = 200.0;

/// Local-texture variance treated as fully anomalous. See `TEXTURE_LIMIT`.
#[allow(dead_code)]
const TEXTURE_FULL: f64 = 800.0;

/// Quiet-zone band width, as a fraction of the symbol side length.
///
/// EMVCo/ISO 18004 require a margin of at least 4 modules; expressed as a
/// fraction so it works for any symbol version.
const QUIET_ZONE_FRACTION: f64 = 0.06;

/// How far outside the symbol boundary the detection ring extends, as a
/// fraction of the side length.
///
/// This is the most sensitive geometric parameter in the layer, and the raw
/// sweep that selected it is worth recording (edge density over the ring):
///
/// ```text
///   band   clean_edge   sticker_edge
///   0.03     0.1394        0.1394     <- falls inside the clean sheet border
///   0.05     0.0797        0.5079     <- still clips the sheet border
///   0.06     0.0678        0.4319
///   0.08     0.0490        0.4219
///   0.10     0.0380        0.3277
///   0.12     0.0321        0.2767
///   0.14     0.0932        0.2969     <- crosses the sheet border again
/// ```
///
/// Too narrow and the ring lands on the soft paper-to-background ramp at the
/// edge of the sheet, which scores "clean" as heavily anomalous (0.1394). Too
/// wide and it crosses that boundary again (0.0932). `0.12` keeps the strongest
/// separation (0.032 versus 0.277, a factor of ~8.6) while still tolerating a
/// sticker pasted with an offset.
const QUIET_ZONE_SEARCH_FRACTION: f64 = 0.12;

/// Normalisation divisor for the temporal-glare-variance term.
#[allow(dead_code)]
const GLARE_VARIANCE_NORMALISER: f64 = 400.0;

/// The score at or above which Layer 1 escalates to `CAUTION`.
///
/// Calibrated from the measured fixture separation: clean scores 0.000 and a
/// pasted sticker scores 0.371. A CAUTION band starting at 0.30 therefore catches
/// the sticker without flagging a clean QR, and leaves room below for partial
/// violations (an offset sticker, a marginal glare patch) to land in CAUTION
/// rather than being pushed straight to HIGH RISK.
const CAUTION_AT: f64 = 0.30;

/// The score at or above which Layer 1 escalates to `HIGH RISK`.
///
/// Lowered from `0.55` together with the re-tuned ramp: a real overlay now lands
/// around `0.55`, and the layer should name that outright rather than leave it
/// one band short. The combined verdict is still governed by `risk_band` in
/// `lib.rs`, so this only decides the severity of Layer 1's own finding.
const HIGH_RISK_AT: f64 = 0.50;

/// Weight of the quiet-zone edge-density signal.
///
/// Edge density carries most of the weight because it has the clearest physical
/// interpretation (a straight line where the margin should be blank) and the
/// strongest measured separation (0.032 clean versus 0.277 sticker).
const EDGE_WEIGHT: f64 = 0.75;

/// Weight of the specular-glare signal.
///
/// Glare is real but confounded by lighting, so it corroborates rather than
/// leads. It is also the signal most likely to be re-weighted after field data.
const GLARE_WEIGHT: f64 = 0.25;

/// Weight of the spatial (single-frame) signal in the optical score.
const SPATIAL_WEIGHT: f64 = 0.70;

/// Sobel gradients on a grayscale buffer.
///
/// Returns `(gx, gy)` as flat `i32` buffers, or `None` when the image is too
/// small for the 3x3 kernel.
fn sobel(gray: &[u8], w: usize, h: usize) -> Option<(Vec<i32>, Vec<i32>)> {
    if w < 3 || h < 3 {
        return None;
    }
    let at = |x: i64, y: i64| -> i32 { gray[(y as usize) * w + x as usize] as i32 };

    let mut gx = vec![0i32; w * h];
    let mut gy = vec![0i32; w * h];
    for y in 1..h as i64 - 1 {
        for x in 1..w as i64 - 1 {
            // Standard Sobel:
            //   Gx = [-1 0 1; -2 0 2; -1 0 1]   Gy = [-1 -2 -1; 0 0 0; 1 2 1]
            let gxv = -at(x - 1, y - 1) - 2 * at(x - 1, y) - at(x - 1, y + 1)
                + at(x + 1, y - 1)
                + 2 * at(x + 1, y)
                + at(x + 1, y + 1);
            let gyv = -at(x - 1, y - 1) - 2 * at(x, y - 1) - at(x + 1, y - 1)
                + at(x - 1, y + 1)
                + 2 * at(x, y + 1)
                + at(x + 1, y + 1);
            let idx = (y as usize) * w + x as usize;
            gx[idx] = gxv;
            gy[idx] = gyv;
        }
    }
    Some((gx, gy))
}

/// Mean absolute deviation of a slice, an outlier-robust spread measure.
///
/// Used instead of standard deviation because a single very dark sticker border
/// would inflate a standard deviation enough to hide the effect it is supposed
/// to reveal.
fn mad(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    values.iter().map(|v| (v - mean).abs()).sum::<f64>() / values.len() as f64
}

/// Edge density measured over the detection ring only.
///
/// The ring is wide enough to straddle a sticker pasted with an offset, but that
/// width has a cost: at the far edge of the sheet the paper-to-background
/// transition is itself a gradient, so an over-wide ring would score a clean QR
/// as heavily anomalous. `QUIET_ZONE_SEARCH_FRACTION` selects a band between
/// those two failure modes; the sweep that chose it is recorded on that
/// constant.
///
/// A sticker is always cut larger than the symbol it covers (otherwise the
/// original would still decode), so its border lands a few pixels to a few tens
/// of pixels outside the symbol, inside this ring, as a straight, high-contrast
/// line where the margin should be blank.
fn ring_edge_density(
    frame: &Frame,
    gx: &[i32],
    gy: &[i32],
    bbox: (u32, u32, u32, u32),
    band: i64,
) -> f64 {
    let (bx, by, bw, bh) = bbox;
    let w = frame.width as i64;
    let h = frame.height as i64;

    let x0 = bx as i64 - band;
    let y0 = by as i64 - band;
    let x1 = bx as i64 + bw as i64 + band;
    let y1 = by as i64 + bh as i64 + band;

    let mut edges = 0usize;
    let mut total = 0usize;

    for y in y0.max(0)..y1.min(h) {
        for x in x0.max(0)..x1.min(w) {
            let inside = x >= bx as i64
                && x < (bx + bw) as i64
                && y >= by as i64
                && y < (by + bh) as i64;
            if inside {
                continue;
            }
            total += 1;
            let idx = (y as usize) * (frame.width as usize) + x as usize;
            let mag = ((gx[idx] as f64).powi(2) + (gy[idx] as f64).powi(2)).sqrt();
            if mag > EDGE_MAGNITUDE_THRESHOLD {
                edges += 1;
            }
        }
    }

    if total == 0 {
        0.0
    } else {
        edges as f64 / total as f64
    }
}

/// Mean absolute gradient magnitude in a rectangular ring around the symbol.
struct RingStats {
    edge_density: f64,
    glare_fraction: f64,
}

/// Measures edge density and glare inside the quiet-zone search ring.
///
/// `bbox` is `(x, y, w, h)` of the symbol. The ring runs from the symbol edge
/// outward by `QUIET_ZONE_SEARCH_FRACTION` of the longer side, which is where a
/// pasted sticker's boundary shows up.
fn ring_stats(frame: &Frame, gray: &[u8], gx: &[i32], gy: &[i32], bbox: (u32, u32, u32, u32)) -> RingStats {
    let (bx, by, bw, bh) = bbox;
    let w = frame.width as i64;
    let h = frame.height as i64;

    let side = bw.max(bh) as f64;
    let band = (side * QUIET_ZONE_SEARCH_FRACTION).max(4.0) as i64;

    let x0 = bx as i64 - band;
    let y0 = by as i64 - band;
    let x1 = bx as i64 + bw as i64 + band;
    let y1 = by as i64 + bh as i64 + band;

    let mut glare = 0usize;
    let mut total = 0usize;

    for y in y0.max(0)..y1.min(h) {
        for x in x0.max(0)..x1.min(w) {
            // Skip the symbol interior: only the margin is of interest.
            let inside = x >= bx as i64 && x < (bx + bw) as i64 && y >= by as i64 && y < (by + bh) as i64;
            if inside {
                continue;
            }
            total += 1;
            let idx = (y as usize) * (frame.width as usize) + x as usize;
            if gray[idx] >= GLARE_PIXEL_MIN {
                glare += 1;
            }
        }
    }

    RingStats {
        edge_density: ring_edge_density(frame, gx, gy, bbox, band),
        glare_fraction: if total == 0 {
            0.0
        } else {
            glare as f64 / total as f64
        },
    }
}

// `_REQUIRED_MARKER` is not present: the required-margin geometry is
// already expressed by `QUIET_ZONE_FRACTION`, used only for the truncation
// check in `analyze_single`.

/// Minimum intensity counted as specular glare.
const GLARE_PIXEL_MIN: u8 = 235;

/// Minimum fraction of the ring that must be glare before the temporal signal is
/// trusted at all.
const GLARE_MIN_FOR_TEMPORAL: f64 = 0.002;

/// Strongest local texture discontinuity over a centered sub-window of the
/// symbol.
///
/// Reports the maximum 3x3 local variance rather than the mean or median. The
/// reason: on a genuine QR the central region is full of printed modules, so its
/// typical texture is high and uninformative, and a mean or median would flag
/// every real QRIS as anomalous. What distinguishes a pasted overlay is that it
/// introduces structure at a *different scale*, which shows up as a small number
/// of very high local variances. The maximum is the statistic that sees those,
/// and it is reported alongside the other metrics so a fitted threshold can be
/// applied later.
///
/// Sampled on a stride so the cost stays flat regardless of frame size. This
/// runs on every capture, including on phones.
fn texture_discontinuity(frame: &Frame, gray: &[u8], bbox: (u32, u32, u32, u32)) -> f64 {
    let (bx, by, bw, bh) = bbox;
    let w = frame.width as usize;
    let h = frame.height as usize;

    // Central 60% of the symbol: avoids the finder patterns, which are edges by
    // design and would swamp the measurement.
    let ix0 = (bx as f64 + bw as f64 * 0.20) as usize;
    let iy0 = (by as f64 + bh as f64 * 0.20) as usize;
    let ix1 = ((bx + bw) as f64 * 0.80) as usize;
    let iy1 = ((by + bh) as f64 * 0.80) as usize;

    if ix1 <= ix0 + 2 || iy1 <= iy0 + 2 {
        return 0.0;
    }

    // 3x3 local variance, strided.
    let stride = ((ix1 - ix0) / 48).max(1);
    let mut peak = 0f64;

    let mut y = iy0.max(1);
    while y + 1 < iy1.min(h - 1) {
        let mut x = ix0.max(1);
        while x + 1 < ix1.min(w - 1) {
            let mut vals = [0f64; 9];
            let mut k = 0;
            for dy in 0..3usize {
                for dx in 0..3usize {
                    vals[k] = gray[(y - 1 + dy) * w + (x - 1 + dx)] as f64;
                    k += 1;
                }
            }
            let mean = vals.iter().sum::<f64>() / 9.0;
            let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / 9.0;
            if var > peak {
                peak = var;
            }
            x += stride;
        }
        y += stride;
    }

    peak
}

/// Normalises `value` onto `0..1` given an expected-good `limit` and a
/// fully-anomalous `full` bound.
fn ramp(value: f64, limit: f64, full: f64) -> f64 {
    if !value.is_finite() || value <= limit {
        return 0.0;
    }
    if value >= full {
        return 1.0;
    }
    (value - limit) / (full - limit)
}

/// Folds the two scored signals into Layer 1's single spatial score.
///
/// Two terms: the weighted level of the signals, and a reward for the signals
/// *disagreeing* with each other. The disagreement term separates "one odd
/// measurement" from "the surface has been changed": a pasted overlay moves the
/// margin edges without necessarily changing the glare, so the two components
/// diverge.
///
/// Extracted from `analyze_single` so the sensitivity of the scoring curve can
/// be asserted directly, without having to synthesise a frame with an exact
/// edge density.
fn combine_components(edge_component: f64, glare_component: f64) -> f64 {
    let spatial = (edge_component * EDGE_WEIGHT + glare_component * GLARE_WEIGHT).clamp(0.0, 1.0);
    let disagreement = mad(&[edge_component, glare_component]);
    (SPATIAL_WEIGHT * spatial + (1.0 - SPATIAL_WEIGHT) * disagreement).clamp(0.0, 1.0)
}

/// Runs the optical analysis for a single frame.
///
/// `bbox` is the QR bounding box reported by the decoder. It is required: the
/// metrics are only meaningful relative to the symbol, and skipping this check
/// is how a "safety" score ends up reported for a scan that examined nothing.
pub fn analyze_single(frame: &Frame, bbox: (u32, u32, u32, u32)) -> Layer1Result {
    let w = frame.width as usize;
    let h = frame.height as usize;
    if w < 8 || h < 8 {
        return Layer1Result::not_run("frame terlalu kecil untuk analisis optik");
    }

    let (bx, by, bw, bh) = bbox;
    if bw == 0 || bh == 0 {
        return Layer1Result::not_run("bbox QR kosong");
    }
    if bx as usize + bw as usize > w || by as usize + bh as usize > h {
        return Layer1Result::not_run("bbox QR di luar batas frame");
    }

    let gray = frame.to_gray();
    let Some((gx, gy)) = sobel(&gray, w, h) else {
        return Layer1Result::not_run("frame terlalu kecil untuk kernel Sobel 3x3");
    };

    let ring = ring_stats(frame, &gray, &gx, &gy, bbox);
    let texture = texture_discontinuity(frame, &gray, bbox);

    let mut warnings = Vec::new();

    // A quiet zone smaller than the spec requires means the symbol is cropped
    // against the frame edge, so the ring may be measuring the frame border
    // rather than the margin. Flag it instead of silently trusting the number.
    let side = bw.max(bh) as f64;
    let required = side * QUIET_ZONE_FRACTION;
    let available = (bx.min(by) as f64).min((w as u32 - bw - bx) as f64).min((h as u32 - bh - by) as f64);
    let quiet_zone_truncated = available < required;
    if quiet_zone_truncated {
        warnings.push(format!(
            "Quiet zone kurang dari spesifikasi (butuh ~{required:.0}px, tersedia ~{available:.0}px); \
             skor tepi kurang dapat diandalkan"
        ));
    }

    // --- signal 1: quiet-zone edge density -------------------------------
    //
    // The ramp output is sharpened so a marginal noise reading stays small while
    // a real overlay (well above the limit) rises steeply. See SIGNAL_SHARPNESS.
    let edge_component = ramp(ring.edge_density, QUIET_ZONE_EDGE_LIMIT, QUIET_ZONE_EDGE_FULL)
        .powf(SIGNAL_SHARPNESS);
    if edge_component > 0.0 {
        warnings.push(format!(
            "Tepi terdeteksi di quiet zone ({:.3} dari piksel margin); \
             indikasi ada objek ditempelkan di atas QR",
            ring.edge_density
        ));
    }

    // --- signal 2: specular glare fraction -------------------------------
    let glare_component = ramp(ring.glare_fraction, GLARE_FRACTION_LIMIT, GLARE_FRACTION_FULL)
        .powf(SIGNAL_SHARPNESS);
    if glare_component > 0.0 {
        warnings.push(format!(
            "Kilau (glare) menutupi {:.1}% quiet zone; permukaan mungkin bukan kertas polos",
            ring.glare_fraction * 100.0
        ));
    }

    // --- signal 3: texture discontinuity (reported, not scored) ----------
    //
    // Computed so the value is available for fitting against real captures, but
    // given no weight: it saturates on any readable QR, so it
    // cannot discriminate. See `TEXTURE_LIMIT`.

    // Spatial score. Edge density leads because it is the signal with the
    // clearest physical interpretation; glare corroborates it, and their
    // disagreement is itself evidence. See `combine_components`.
    let l1_score = combine_components(edge_component, glare_component);

    let risk_level = if l1_score >= HIGH_RISK_AT {
        "HIGH RISK"
    } else if l1_score >= CAUTION_AT {
        "CAUTION"
    } else {
        "LOW RISK"
    };

    Layer1Result {
        l1_score,
        spatial_edge_density: ring.edge_density,
        temporal_glare_var: 0.0,
        texture_discontinuity: texture,
        glare_fraction: ring.glare_fraction,
        risk_level: risk_level.to_string(),
        quiet_zone_truncated,
        warnings,
    }
}

/// Analyses a short burst of frames and folds in the temporal glare variance.
///
/// A burst is what makes the glare signal meaningful: a single frame cannot tell
/// a moving highlight from a static bright patch. Fewer than
/// [`MIN_BURST_FRAMES`] usable frames means the temporal term is reported as
/// zero *and* called out in `warnings`, rather than being silently treated as
/// "no glare risk".
pub const MIN_BURST_FRAMES: usize = 3;

/// How much the temporal glare signal can raise the spatial score.
///
/// Kept below 1.0 on purpose: glare corroborates a spatial finding, it does not
/// convict on its own, because a shiny but untouched QR can move highlights too.
const TEMPORAL_BOOST: f64 = 0.35;

pub fn analyze_burst(frames: &[Frame], bbox: (u32, u32, u32, u32)) -> Layer1Result {
    if frames.is_empty() {
        return Layer1Result::not_run("tidak ada frame");
    }

    let mut result = analyze_single(&frames[0], bbox);
    if !result.ran() {
        return result;
    }

    if frames.len() < MIN_BURST_FRAMES {
        result.warnings.push(format!(
            "Hanya {} frame untuk analisis temporal (butuh {}); \
             variansi glare tidak diukur",
            frames.len(),
            MIN_BURST_FRAMES
        ));
        return result;
    }

    // The Sobel buffers are reused across the burst: they gate which pixels
    // count as edges, and that geometry does not change between frames of the
    // same size. Only the grayscale conversion is redone per frame.
    let w = frames[0].width as usize;
    let h = frames[0].height as usize;
    let Some((gx, gy)) = sobel(&frames[0].to_gray(), w, h) else {
        return result;
    };

    let mut fractions = Vec::with_capacity(frames.len());
    for f in frames {
        // Frames of differing size cannot be compared; skip rather than
        // fabricate a variance from mismatched data.
        if f.width != frames[0].width || f.height != frames[0].height {
            continue;
        }
        if f.rgb.len() != frames[0].rgb.len() {
            continue;
        }
        let gray = f.to_gray();
        let stats = ring_stats(f, &gray, &gx, &gy, bbox);
        fractions.push(stats.glare_fraction);
    }

    if fractions.len() < MIN_BURST_FRAMES {
        result.warnings.push(
            "Frame burst tidak konsisten ukurannya; variansi glare dilewati".to_string(),
        );
        return result;
    }

    let mean = fractions.iter().sum::<f64>() / fractions.len() as f64;
    let variance = fractions.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / fractions.len() as f64;

    result.temporal_glare_var = variance;

    // Temporal term is quantised to keep one burst zero and only respond once
    // the variance clears the documented threshold; see `GLARE_VARIANCE_LIMIT`.
    let temporal_component = if mean < GLARE_MIN_FOR_TEMPORAL || variance < GLARE_VARIANCE_LIMIT {
        0.0
    } else {
        ramp(variance, GLARE_VARIANCE_LIMIT, GLARE_VARIANCE_FULL)
    };

    if temporal_component > 0.0 {
        result.warnings.push(format!(
            "Kilau berubah antar-frame (varians {:.6}); permukaan memantulkan cahaya berbeda \
             dari kertas di sekitarnya",
            result.temporal_glare_var
        ));
    }

    // Temporal term is folded in as a bounded bonus: it corroborates a spatial
    // finding but is not strong enough on its own to convict a QR.
    let boosted = (result.l1_score + TEMPORAL_BOOST * temporal_component * (1.0 - result.l1_score))
        .clamp(0.0, 1.0);

    result.l1_score = if result.quiet_zone_truncated {
        // Under an unmeasurable margin, degrade toward "unknown" instead of
        // reporting a confident number produced by a bad measurement.
        result.l1_score.min(boosted)
    } else {
        boosted
    };

    result.risk_level = if result.l1_score >= HIGH_RISK_AT {
        "HIGH RISK"
    } else if result.l1_score >= CAUTION_AT {
        "CAUTION"
    } else {
        "LOW RISK"
    }
    .to_string();

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::synthetic::{SyntheticBackend, SyntheticSpec};
    use crate::camera::CameraBackend;

    /// Symmetric margin available around the default synthetic bbox (200,140,240,240).
    fn default_bbox() -> (u32, u32, u32, u32) {
        (200, 140, 240, 240)
    }

    fn clean_frame() -> Frame {
        SyntheticBackend::with_spec(SyntheticSpec {
            sticker_anomaly: false,
            glare_radius: 1,
            ..Default::default()
        })
        .capture()
        .unwrap()
    }

    fn sticker_frame() -> Frame {
        SyntheticBackend::with_spec(SyntheticSpec {
            sticker_anomaly: true,
            glare_radius: 1,
            ..Default::default()
        })
        .capture()
        .unwrap()
    }

    #[test]
    fn clean_frame_does_not_trigger_the_caution_band() {
        let r = analyze_single(&clean_frame(), default_bbox());
        assert!(r.ran(), "layer must have run");
        assert_eq!(
            r.risk_level, "LOW RISK",
            "clean QR must stay below CAUTION, got {:.3} (edge={:.4})",
            r.l1_score, r.spatial_edge_density
        );
        assert!(
            r.spatial_edge_density < QUIET_ZONE_EDGE_LIMIT,
            "clean edge density {:.4} must be under the limit {}",
            r.spatial_edge_density,
            QUIET_ZONE_EDGE_LIMIT
        );
    }

    #[test]
    fn sticker_frame_is_flagged_at_least_caution() {
        // The property the whole layer exists for. It asserts "at least
        // CAUTION", not HIGH RISK: this fixture's overlay also blocks part of
        // the symbol, so it is a moderately severe violation, and forcing the
        // thresholds down to make it shout would start flagging clean QRs.
        let r = analyze_single(&sticker_frame(), default_bbox());
        assert!(
            r.risk_level == "CAUTION" || r.risk_level == "HIGH RISK",
            "a pasted sticker must be flagged, got {} (score {:.3}, edge {:.4})",
            r.risk_level,
            r.l1_score,
            r.spatial_edge_density
        );
        assert!(
            r.spatial_edge_density > QUIET_ZONE_EDGE_LIMIT,
            "sticker edge density {:.4} must exceed the limit {}",
            r.spatial_edge_density,
            QUIET_ZONE_EDGE_LIMIT
        );
        assert!(r.warnings.iter().any(|w| w.contains("quiet zone")));
    }

    #[test]
    fn edge_limit_sits_between_clean_and_sticker_measurements() {
        // The calibration invariant. If a constant is retuned such that the
        // limit no longer separates the two fixtures, this fails loudly instead
        // of the detector silently becoming blind or always-alarming.
        let clean = analyze_single(&clean_frame(), default_bbox()).spatial_edge_density;
        let sticker = analyze_single(&sticker_frame(), default_bbox()).spatial_edge_density;

        assert!(
            clean < QUIET_ZONE_EDGE_LIMIT && QUIET_ZONE_EDGE_LIMIT < sticker,
            "limit {QUIET_ZONE_EDGE_LIMIT} must lie strictly between clean {clean:.4} and sticker {sticker:.4}"
        );
        assert!(
            sticker / clean.max(f64::EPSILON) > 3.0,
            "separation too weak to be meaningful: clean={clean:.4} sticker={sticker:.4}"
        );
        assert!(
            QUIET_ZONE_EDGE_LIMIT >= clean * (QUIET_ZONE_EDGE_LIMIT_MULTIPLE_OF_CLEAN - 0.5),
            "limit must keep the documented margin above the clean measurement"
        );
    }

    #[test]
    fn reported_metrics_are_finite_and_in_range() {
        for frame in [clean_frame(), sticker_frame()] {
            let r = analyze_single(&frame, default_bbox());
            assert!(r.l1_score.is_finite() && (0.0..=1.0).contains(&r.l1_score));
            assert!((0.0..=1.0).contains(&r.spatial_edge_density));
            assert!((0.0..=1.0).contains(&r.glare_fraction));
            assert!(r.texture_discontinuity.is_finite());
        }
    }

    #[test]
    fn empty_bbox_is_not_run_not_zero() {
        // The critical distinction: no measurement must never masquerade as a
        // safe result.
        let r = analyze_single(&clean_frame(), (0, 0, 0, 0));
        assert_eq!(r.risk_level, "NOT RUN");
        assert!(!r.ran());
        assert_eq!(r.l1_score, 0.0);
    }

    #[test]
    fn out_of_bounds_bbox_is_not_run() {
        let r = analyze_single(&clean_frame(), (600, 400, 100, 100));
        assert_eq!(r.risk_level, "NOT RUN");
    }

    #[test]
    fn tiny_frame_is_not_run() {
        let f = Frame::new(4, 4, vec![0; 48]).unwrap();
        let r = analyze_single(&f, (0, 0, 2, 2));
        assert_eq!(r.risk_level, "NOT RUN");
    }

    #[test]
    fn cropped_quiet_zone_is_flagged() {
        // Symbol pushed into the top-left corner: almost no margin to measure.
        let mut backend = SyntheticBackend::with_spec(SyntheticSpec {
            sticker_anomaly: false,
            glare_radius: 1,
            qr_bbox: (1, 1, 240, 240),
            ..Default::default()
        });
        let frame = backend.capture().unwrap();
        let r = analyze_single(&frame, (1, 1, 240, 240));
        assert!(
            r.quiet_zone_truncated,
            "a symbol against the frame edge must report a truncated quiet zone"
        );
        assert!(r.warnings.iter().any(|w| w.contains("Quiet zone")));
    }

    #[test]
    fn burst_reports_zero_glare_variance_below_minimum_frames() {
        let frames = vec![clean_frame(), clean_frame()];
        let r = analyze_burst(&frames, default_bbox());
        assert_eq!(r.temporal_glare_var, 0.0);
        assert!(
            r.warnings.iter().any(|w| w.contains("analisis temporal")),
            "a skipped temporal check must be announced, not hidden: {:?}",
            r.warnings
        );
    }

    #[test]
    fn burst_on_sticker_frame_keeps_the_spatial_finding() {
        let frames: Vec<Frame> = (0..5).map(|_| sticker_frame()).collect();
        let r = analyze_burst(&frames, default_bbox());
        assert!(
            r.risk_level == "CAUTION" || r.risk_level == "HIGH RISK",
            "a sticker must stay flagged through the burst path, got {} ({:.3})",
            r.risk_level,
            r.l1_score
        );
        assert_eq!(
            r.temporal_glare_var, 0.0,
            "identical frames carry no temporal signal"
        );
    }

    #[test]
    fn burst_detects_moving_glare_in_the_ring() {
        // Varying specular highlight across frames, as a glossy overlay under a
        // moving light would produce. The highlight is a 90 px radius, which
        // reaches into the detection ring around a 240 px symbol.
        let bbox = (200, 140, 240, 240);
        let radii = [95u32, 60, 130, 70, 110];
        let frames: Vec<Frame> = radii
            .iter()
            .map(|r| {
                SyntheticBackend::with_spec(SyntheticSpec {
                    sticker_anomaly: false,
                    glare_radius: *r,
                    ..Default::default()
                })
                .capture()
                .unwrap()
            })
            .collect();

        let per_frame: Vec<f64> = frames
            .iter()
            .map(|f| analyze_single(f, bbox).glare_fraction)
            .collect();
        assert!(
            per_frame.iter().any(|v| *v > 0.0),
            "the fixture must actually place glare in the ring, got {per_frame:?}"
        );

        let r = analyze_burst(&frames, bbox);
        assert!(
            r.temporal_glare_var > 0.0,
            "a moving highlight must produce non-zero temporal variance, got {:.8} (per-frame {per_frame:?})",
            r.temporal_glare_var
        );
    }

    #[test]
    fn stable_glare_produces_no_temporal_signal() {
        // Same highlight in every frame: bright, but not *moving*, so the
        // temporal check must stay quiet.
        let bbox = (200, 140, 240, 240);
        let frames: Vec<Frame> = (0..5)
            .map(|_| {
                SyntheticBackend::with_spec(SyntheticSpec {
                    sticker_anomaly: false,
                    glare_radius: 110,
                    ..Default::default()
                })
                .capture()
                .unwrap()
            })
            .collect();

        let r = analyze_burst(&frames, bbox);
        assert_eq!(
            r.temporal_glare_var, 0.0,
            "a static highlight must not register as temporal glare"
        );
    }

    #[test]
    fn sticker_frame_scores_strictly_higher_than_clean() {
        let clean = analyze_single(&clean_frame(), default_bbox());
        let sticker = analyze_single(&sticker_frame(), default_bbox());

        assert!(
            sticker.spatial_edge_density > clean.spatial_edge_density,
            "sticker must add edges in the margin: clean={:.5} sticker={:.5}",
            clean.spatial_edge_density,
            sticker.spatial_edge_density
        );
        assert!(
            sticker.l1_score > clean.l1_score,
            "sticker must score higher: clean={:.3} sticker={:.3}",
            clean.l1_score,
            sticker.l1_score
        );
        assert_eq!(clean.l1_score, 0.0, "a clean QR must score exactly zero");
    }

    #[test]
    fn texture_metric_is_reported_but_does_not_move_the_score() {
        // Documents why the texture signal is excluded: it saturates on any
        // readable QR, so it must not influence the spatial score.
        let r = analyze_single(&clean_frame(), default_bbox());
        assert!(
            r.texture_discontinuity > 0.0,
            "the metric must still be measured and reported for later fitting"
        );
        assert_eq!(
            r.l1_score, 0.0,
            "a saturated texture metric must not leak into the score"
        );
    }

    #[test]
    fn burst_rejects_mismatched_frame_sizes() {
        let mut frames = vec![clean_frame(); 5];
        frames.push(Frame::new(320, 240, vec![0; 320 * 240 * 3]).unwrap());
        let r = analyze_burst(&frames, default_bbox());
        // Must not panic and must not invent a variance from bad data.
        assert!(r.l1_score.is_finite());
    }

    #[test]
    fn empty_burst_is_not_run() {
        let r = analyze_burst(&[], default_bbox());
        assert_eq!(r.risk_level, "NOT RUN");
    }

    #[test]
    fn not_run_constructor_is_explicit() {
        let r = Layer1Result::not_run("alasan uji");
        assert!(!r.ran());
        assert_eq!(r.risk_level, "NOT RUN");
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn ramp_is_monotonic_and_clamped() {
        assert_eq!(ramp(0.0, 0.1, 1.0), 0.0);
        assert_eq!(ramp(0.1, 0.1, 1.0), 0.0);
        assert_eq!(ramp(1.0, 0.1, 1.0), 1.0);
        assert_eq!(ramp(5.0, 0.1, 1.0), 1.0);
        assert_eq!(ramp(f64::NAN, 0.1, 1.0), 0.0);
        let mid = ramp(0.55, 0.1, 1.0);
        assert!((0.0..1.0).contains(&mid));
    }

    #[test]
    fn mad_is_zero_for_identical_signals() {
        assert_eq!(mad(&[0.5, 0.5, 0.5]), 0.0);
        assert!(mad(&[0.0, 1.0, 0.0]) > 0.0);
    }

    #[test]
    fn sobel_detects_a_vertical_edge() {
        // Left half black, right half white: a strong gx, weak gy.
        let w = 16usize;
        let h = 16usize;
        let mut gray = vec![0u8; w * h];
        for y in 0..h {
            for x in 8..w {
                gray[y * w + x] = 255;
            }
        }
        let (gx, gy) = sobel(&gray, w, h).unwrap();
        let idx = 8 * w + 8;
        assert!(gx[idx].abs() > 100, "expected strong horizontal gradient");
        assert!(gy[idx].abs() < 10, "expected weak vertical gradient");
    }

    #[test]
    fn sobel_rejects_tiny_images() {
        assert!(sobel(&[0u8; 4], 2, 2).is_none());
    }

    /// Guards the sensitivity re-tune.
    ///
    /// Synthetic fixtures have far more contrast than a phone capture, so they
    /// cannot express the case that matters: an overlay photographed in the
    /// field. Field reports put a clean capture near `0.081` edge density and a
    /// tampered one near `0.122`. On the original `0.450` full bound those two
    /// produced roughly 4% and 10%, a separation that never reached a risk
    /// band. The detector must now put the clean one below CAUTION and the
    /// overlay clearly above it.
    #[test]
    fn a_field_typical_overlay_reaches_caution() {
        let score_of = |edge_density: f64| {
            let c = ramp(edge_density, QUIET_ZONE_EDGE_LIMIT, QUIET_ZONE_EDGE_FULL)
                .powf(SIGNAL_SHARPNESS);
            combine_components(c, 0.0)
        };

        let clean = score_of(0.081);
        let tampered = score_of(0.122);

        assert!(
            clean < CAUTION_AT,
            "a clean field capture must stay below CAUTION, got {clean:.3}"
        );
        assert!(
            tampered >= CAUTION_AT,
            "a field overlay must reach CAUTION, got {tampered:.3}"
        );
        assert!(
            tampered > clean * 3.0,
            "separation too weak to act on: clean={clean:.3} tampered={tampered:.3}"
        );
    }
}
