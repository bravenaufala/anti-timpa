//! Synthetic camera backend.
//!
//! Generates deterministic frames containing a QR-like pattern with an
//! optional sticker anomaly and glare. This is the direct successor to the
//! synthetic generator in `main_mobile.py`, and crucially makes the whole
//! camera -> analysis pipeline unit-testable without hardware.

use super::{CameraBackend, CameraError, Frame};

/// Describes what the synthetic frame should look like.
#[derive(Debug, Clone)]
pub struct SyntheticSpec {
    pub width: u32,
    pub height: u32,
    /// Quiet-zone bounding box of the fake QR code (x, y, w, h).
    pub qr_bbox: (u32, u32, u32, u32),
    /// Draws a high-contrast double edge in the quiet zone margin,
    /// simulating a sticker pasted over the real QR.
    pub sticker_anomaly: bool,
    /// Radius of a bright specular highlight. Varies frame-to-frame to
    /// exercise the Layer 1 temporal glare variance.
    pub glare_radius: u32,
}

impl Default for SyntheticSpec {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            // Matches the geometry used by `test_layer1.py` so the same
            // expectations hold in both languages.
            qr_bbox: (200, 140, 240, 240),
            sticker_anomaly: false,
            glare_radius: 0,
        }
    }
}

pub struct SyntheticBackend {
    spec: SyntheticSpec,
    ready: bool,
    /// Cycles glare sizes so consecutive captures produce a non-zero
    /// variance, which is what the temporal Layer 1 check looks for.
    tick: usize,
}

impl Default for SyntheticBackend {
    fn default() -> Self {
        Self {
            spec: SyntheticSpec::default(),
            ready: true,
            tick: 0,
        }
    }
}

impl SyntheticBackend {
    pub fn new(glare_cycle: bool) -> Self {
        Self {
            spec: SyntheticSpec::default(),
            ready: true,
            tick: if glare_cycle { 0 } else { usize::MAX },
        }
    }

    pub fn with_spec(spec: SyntheticSpec) -> Self {
        Self {
            spec,
            ready: true,
            tick: 0,
        }
    }

    /// Fills a rectangle, clipping to frame bounds.
    fn fill_rect(
        rgb: &mut [u8],
        width: u32,
        height: u32,
        x: i64,
        y: i64,
        w: i64,
        h: i64,
        color: [u8; 3],
    ) {
        let x0 = x.clamp(0, width as i64);
        let y0 = y.clamp(0, height as i64);
        let x1 = (x + w).clamp(0, width as i64);
        let y1 = (y + h).clamp(0, height as i64);

        for py in y0..y1 {
            for px in x0..x1 {
                let idx = ((py as u32 * width + px as u32) * 3) as usize;
                rgb[idx..idx + 3].copy_from_slice(&color);
            }
        }
    }

    /// Draws a circle filled with `color`, clipped to bounds.
    fn fill_circle(
        rgb: &mut [u8],
        width: u32,
        height: u32,
        cx: i64,
        cy: i64,
        radius: i64,
        color: [u8; 3],
    ) {
        let r2 = radius * radius;
        let y0 = (cy - radius).max(0);
        let y1 = (cy + radius).min(height as i64);
        let x0 = (cx - radius).max(0);
        let x1 = (cx + radius).min(width as i64);

        for py in y0..y1 {
            for px in x0..x1 {
                let dx = px - cx;
                let dy = py - cy;
                if dx * dx + dy * dy <= r2 {
                    let idx = ((py as u32 * width + px as u32) * 3) as usize;
                    rgb[idx..idx + 3].copy_from_slice(&color);
                }
            }
        }
    }
}

impl CameraBackend for SyntheticBackend {
    fn name(&self) -> &'static str {
        "synthetic"
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn capture(&mut self) -> Result<Frame, CameraError> {
        if !self.ready {
            return Err(CameraError::CaptureFailed {
                reason: "backend sintetik tidak aktif".into(),
            });
        }

        let w = self.spec.width;
        let h = self.spec.height;
        let (qx, qy, qw, qh) = self.spec.qr_bbox;

        // Off-white paper background (deliberately below the glare threshold
        // of 220 so only the synthetic highlight counts as glare).
        let mut rgb = vec![195u8; (w * h * 3) as usize];

        // 1. Quiet zone paper margin.
        Self::fill_rect(
            &mut rgb,
            w,
            h,
            qx as i64 - 30,
            qy as i64 - 30,
            qw as i64 + 60,
            qh as i64 + 60,
            [210, 210, 210],
        );

        // 2. QR body outline plus three finder patterns.
        Self::fill_rect(
            &mut rgb,
            w,
            h,
            qx as i64,
            qy as i64,
            qw as i64,
            2,
            [0, 0, 0],
        );
        Self::fill_rect(
            &mut rgb,
            w,
            h,
            qx as i64,
            qy as i64 + qh as i64 - 2,
            qw as i64,
            2,
            [0, 0, 0],
        );
        Self::fill_rect(
            &mut rgb,
            w,
            h,
            qx as i64,
            qy as i64,
            2,
            qh as i64,
            [0, 0, 0],
        );
        Self::fill_rect(
            &mut rgb,
            w,
            h,
            qx as i64 + qw as i64 - 2,
            qy as i64,
            2,
            qh as i64,
            [0, 0, 0],
        );
        for (fx, fy) in [(10, 10), (qw as i64 - 60, 10), (10, qh as i64 - 60)] {
            Self::fill_rect(&mut rgb, w, h, qx as i64 + fx, qy as i64 + fy, 50, 50, [0, 0, 0]);
        }

        // 3. Sticker anomaly: high-contrast parallel edges in the 10% margin.
        if self.spec.sticker_anomaly {
            let x = qx as i64 - 15;
            let y = qy as i64 - 15;
            let ww = qw as i64 + 30;
            let hh = qh as i64 + 30;
            // Dark frame, 4px thick.
            for t in 0..4 {
                Self::fill_rect(&mut rgb, w, h, x, y + t, ww, 1, [30, 30, 30]);
                Self::fill_rect(&mut rgb, w, h, x, y + hh - 1 - t, ww, 1, [30, 30, 30]);
                Self::fill_rect(&mut rgb, w, h, x + t, y, 1, hh, [30, 30, 30]);
                Self::fill_rect(&mut rgb, w, h, x + ww - 1 - t, y, 1, hh, [30, 30, 30]);
            }
            // Lighter inner echo, 2px, creating the second edge pair.
            for t in 0..2 {
                Self::fill_rect(&mut rgb, w, h, x + 5, y + 5 + t, ww - 10, 1, [180, 180, 180]);
                Self::fill_rect(&mut rgb, w, h, x + 5, y + hh - 6 - t, ww - 10, 1, [180, 180, 180]);
                Self::fill_rect(&mut rgb, w, h, x + 5 + t, y + 5, 1, hh - 10, [180, 180, 180]);
                Self::fill_rect(&mut rgb, w, h, x + ww - 6 - t, y + 5, 1, hh - 10, [180, 180, 180]);
            }
        }

        // 4. Specular glare (> 220), varying per capture to create variance.
        let glare = match self.spec.glare_radius {
            0 => self.cycle_glare(),
            fixed => fixed,
        };
        if glare > 0 {
            Self::fill_circle(
                &mut rgb,
                w,
                h,
                qx as i64 + qw as i64 / 2,
                qy as i64 + qh as i64 / 2,
                glare as i64,
                [255, 255, 255],
            );
        }

        self.tick = self.tick.wrapping_add(1);
        Frame::new(w, h, rgb)
    }

    fn release(&mut self) {
        self.ready = false;
    }
}

impl SyntheticBackend {
    /// Cycles through glare radii [0, 40, 10, 50, 5], matching the sequence in
    /// `test_layer1.py::test_temporal_glare_variance`. Single-frame callers
    /// therefore still see a plausible image, while repeated captures produce
    /// a glare variance the temporal Layer 1 check can detect.
    fn cycle_glare(&self) -> u32 {
        const CYCLE: [u32; 5] = [0, 40, 10, 50, 5];
        CYCLE[self.tick % CYCLE.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_has_expected_dimensions_and_length() {
        let mut backend = SyntheticBackend::default();
        let frame = backend.capture().unwrap();
        assert_eq!((frame.width, frame.height), (640, 480));
        assert_eq!(frame.rgb.len(), 640 * 480 * 3);
    }

    #[test]
    fn background_is_below_glare_threshold() {
        // The paper background must stay under 220, otherwise every synthetic
        // frame would register as glare and the temporal check would be
        // meaningless.
        let mut backend = SyntheticBackend::default();
        let frame = backend.capture().unwrap();
        let max_bg = frame.rgb[0];
        assert!(max_bg < 220, "background {max_bg} must be below glare threshold");
    }

    #[test]
    fn sticker_anomaly_adds_dark_pixels_in_margin() {
        let clean = SyntheticBackend::with_spec(SyntheticSpec {
            sticker_anomaly: false,
            ..Default::default()
        })
        .capture()
        .unwrap();

        let sticker = SyntheticBackend::with_spec(SyntheticSpec {
            sticker_anomaly: true,
            ..Default::default()
        })
        .capture()
        .unwrap();

        let dark = |f: &Frame| f.rgb.chunks_exact(3).filter(|p| p[0] < 50).count();
        assert!(
            dark(&sticker) > dark(&clean),
            "sticker frame must contain more dark pixels than a clean one"
        );
    }

    #[test]
    fn glare_radius_creates_bright_pixels() {
        let with_glare = SyntheticBackend::with_spec(SyntheticSpec {
            glare_radius: 40,
            ..Default::default()
        })
        .capture()
        .unwrap();

        let bright = with_glare
            .rgb
            .chunks_exact(3)
            .filter(|p| p[0] > 220)
            .count();
        assert!(bright > 0, "glare radius 40 should produce bright pixels");
    }

    #[test]
    fn glare_cycle_varies_across_captures() {
        // This is the behaviour Layer 1's temporal glare variance depends on.
        let mut backend = SyntheticBackend::new(true);
        let counts: Vec<usize> = (0..5)
            .map(|_| {
                let f = backend.capture().unwrap();
                f.rgb.chunks_exact(3).filter(|p| p[0] > 220).count()
            })
            .collect();

        let distinct: std::collections::HashSet<_> = counts.iter().collect();
        assert!(
            distinct.len() > 1,
            "glare should vary across captures, got {counts:?}"
        );
    }

    #[test]
    fn release_stops_capture() {
        let mut backend = SyntheticBackend::default();
        assert!(backend.capture().is_ok());
        backend.release();
        assert!(!backend.is_ready());
        assert!(backend.capture().is_err());
    }

    #[test]
    fn to_gray_matches_rec601_luma() {
        // Pure red under Rec. 601 is 0.299 * 255 = 76.
        let frame = Frame::new(1, 1, vec![255, 0, 0]).unwrap();
        assert_eq!(frame.to_gray(), vec![76]);
    }
}
