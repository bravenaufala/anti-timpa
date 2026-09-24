//! Shareable scan report.
//!
//! The point of a scan is to *do* something with the result. That usually means
//! showing it to a merchant, handing it to a bank's dispute desk, or filing it
//! with a report to the authorities. All of those need the same thing: a plain
//! document that states what was checked, what was found, and — importantly —
//! what was **not** checked.
//!
//! Two deliberate design choices:
//!
//! * The report always lists the layers that did **not** run. A report that
//!   omits "optical tamper analysis: not run" is a report that overstates its
//!   own evidence, which is worse than no report at all.
//!
//! * No network, no upload. The caller gets a string and decides what to do with
//!   it (save, copy, share). The app's "100% on-device" guarantee is intact.

use crate::ScanSnapshot;
use serde::{Deserialize, Serialize};

/// Report formats the backend can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportFormat {
    /// Human-readable plain text, suitable for copy-paste into a chat or email.
    Text,
    /// Single-sheet HTML (self-contained, no external assets) for printing/PDF.
    Html,
}

impl ReportFormat {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "html" => ReportFormat::Html,
            _ => ReportFormat::Text,
        }
    }
}

/// Everything the report needs, gathered so generation is a pure function and
/// therefore testable without a Tauri context.
#[derive(Debug, Clone)]
pub struct ReportInput<'a> {
    pub snapshot: &'a ScanSnapshot,
    pub source: &'a str,
    pub timestamp_ms: Option<u64>,
    pub app_version: &'a str,
    /// Integrity hash of the corresponding history entry, when available.
    pub chain_hash: Option<u64>,
}

/// One "what was checked" line, so the report can be explicit about coverage.
struct Coverage {
    label: &'static str,
    status: &'static str,
    detail: String,
}

fn coverage(snapshot: &ScanSnapshot) -> Vec<Coverage> {
    let l1_ran = snapshot
        .l1
        .get("risk_level")
        .and_then(|v| v.as_str())
        .map(|s| s != "NOT RUN")
        .unwrap_or(false);

    let mut out = vec![Coverage {
        label: "L1 Optical tamper (quiet-zone edge density, glare variance)",
        status: if l1_ran { "RAN" } else { "NOT RUN" },
        detail: if l1_ran {
            format!(
                "edge_density={:.4}, glare_var={:.4}",
                snapshot.l1.get("spatial_edge_density").and_then(|v| v.as_f64()).unwrap_or(0.0),
                snapshot.l1.get("temporal_glare_var").and_then(|v| v.as_f64()).unwrap_or(0.0),
            )
        } else {
            "no frame was available to the optical pipeline".to_string()
        },
    }];

    out.push(Coverage {
        label: "L2 EMVCo payload integrity (TLV, CRC-16/CCITT-FALSE, MCC, currency)",
        status: "RAN",
        detail: format!(
            "crc={}",
            if snapshot.l2.get("crc_valid").and_then(|v| v.as_bool()).unwrap_or(false) {
                "valid"
            } else {
                "FAILED"
            }
        ),
    });

    let l3_ran = snapshot
        .l3
        .get("warnings")
        .and_then(|v| v.as_array())
        .map(|w| {
            !w.iter().any(|s| {
                s.as_str()
                    .map(|t| t.contains("skipped geofence") || t.contains("unavailable"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    out.push(Coverage {
        label: "L3 Geofence (client city vs merchant city)",
        status: if l3_ran { "RAN" } else { "NOT RUN" },
        detail: if l3_ran {
            format!(
                "{} vs {}",
                snapshot
                    .l3
                    .get("client_city")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?"),
                snapshot
                    .l3
                    .get("merchant_city")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
            )
        } else {
            "client location was not supplied, so the comparison was skipped".to_string()
        },
    });

    out
}

fn warnings(snapshot: &ScanSnapshot) -> Vec<String> {
    let mut all = Vec::new();
    for layer in [&snapshot.l2, &snapshot.l3, &snapshot.l1] {
        if let Some(arr) = layer.get("warnings").and_then(|v| v.as_array()) {
            for w in arr {
                if let Some(s) = w.as_str() {
                    all.push(s.to_string());
                }
            }
        }
    }
    all
}

fn str_field<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

fn f_field(v: &serde_json::Value, key: &str) -> f64 {
    v.get(key).and_then(|x| x.as_f64()).unwrap_or(0.0)
}

fn bool_field(v: &serde_json::Value, key: &str) -> bool {
    v.get(key).and_then(|x| x.as_bool()).unwrap_or(false)
}

/// Renders the `timestamp_ms` as a UTC timestamp, or an explicit placeholder.
///
/// Implemented without a date library: the report only needs an unambiguous
/// instant, and an epoch value is unambiguous even if it is less friendly.
fn timestamp_line(ms: Option<u64>) -> String {
    match ms {
        Some(ms) => format!("{ms} (epoch ms, UTC)"),
        None => "unavailable (platform clock not supplied)".to_string(),
    }
}

/// Plain-text report.
pub fn render_text(input: &ReportInput) -> String {
    let s = input.snapshot;
    let mut out = String::new();

    out.push_str("==================================================\n");
    out.push_str("  LAPORAN PEMINDAIAN QRIS - ANTI TIMPA\n");
    out.push_str("==================================================\n\n");

    out.push_str(&format!("Waktu        : {}\n", timestamp_line(input.timestamp_ms)));
    out.push_str(&format!("Versi aplikasi: {}\n", input.app_version));
    out.push_str(&format!("Sumber       : {}\n", input.source));
    if let Some(h) = input.chain_hash {
        out.push_str(&format!("Hash rantai  : {h:#018x}\n"));
    }
    out.push('\n');

    out.push_str("--- HASIL ---\n");
    out.push_str(&format!("Skor gabungan : {:.2}\n", s.combined_score));
    out.push_str(&format!("Tingkat risiko: {}\n\n", s.combined_risk_level));

    out.push_str("--- CAKUPAN PEMERIKSAAN ---\n");
    for c in coverage(s) {
        out.push_str(&format!("[{}] {}\n", c.status, c.label));
        out.push_str(&format!("    {}\n", c.detail));
    }
    out.push('\n');

    out.push_str("--- RINCIAN PAYLOAD (EMVCo TLV) ---\n");
    let l2 = &s.l2;
    out.push_str(&format!("Merchant      : {}\n", str_field(l2, "merchant_name")));
    out.push_str(&format!("Kota merchant : {}\n", str_field(l2, "merchant_city")));
    out.push_str(&format!("Kota klien    : {}\n", str_field(&s.l3, "client_city")));
    out.push_str(&format!("MCC           : {}\n", str_field(l2, "mcc")));
    out.push_str(&format!(
        "Mode inisiasi : {}\n",
        match str_field(l2, "initiation_mode") {
            "11" => "11 (statis)",
            "12" => "12 (dinamis)",
            other => other,
        }
    ));
    out.push_str(&format!(
        "CRC-16        : {}\n",
        if bool_field(l2, "crc_valid") { "VALID" } else { "GAGAL" }
    ));
    if s.blur_var > 0.0 {
        out.push_str(&format!(
            "Ketajaman     : {:.1} (Laplacian variance, ambang 100)\n",
            s.blur_var
        ));
    }
    out.push('\n');

    let w = warnings(s);
    out.push_str("--- TEMUAN ---\n");
    if w.is_empty() {
        out.push_str("Tidak ada aturan risiko yang terpicu.\n");
    } else {
        for (i, item) in w.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", i + 1, item));
        }
    }
    out.push('\n');

    if !s.raw_qris_str.is_empty() {
        out.push_str("--- PAYLOAD ---\n");
        out.push_str(&s.raw_qris_str);
        out.push_str("\n\n");
    }

    out.push_str("--- CATATAN ---\n");
    out.push_str(
        "Laporan ini dihasilkan di perangkat, tanpa koneksi jaringan.\n\
         Ketiga lapisan menganalisis PAYLOAD dan GAMBAR, bukan identitas pemilik QRIS.\n\
         Hasil \"LOW RISK\" berarti tidak ada anomali yang terdeteksi, bukan jaminan\n\
         bahwa QRIS tersebut sah. Verifikasi ke merchant sebelum bertransaksi.\n",
    );

    out
}

/// Escapes the five HTML-significant characters.
///
/// The report embeds values that came from a QR code, which is attacker
/// controlled. Without escaping, a merchant name containing markup would be
/// injected straight into the rendered document.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// Self-contained single-sheet HTML report.
///
/// No external stylesheet, font, or script: the document must render identically
/// offline, since it is generated and consumed on-device.
pub fn render_html(input: &ReportInput) -> String {
    let s = input.snapshot;

    let band_class = match s.combined_risk_level.as_str() {
        "HIGH RISK" => "high",
        "CAUTION" => "caution",
        _ => "low",
    };

    let mut cov_rows = String::new();
    for c in coverage(s) {
        let cls = if c.status == "RAN" { "ran" } else { "notrun" };
        cov_rows.push_str(&format!(
            "<tr><td class=\"{cls}\">{}</td><td>{}</td><td>{}</td></tr>",
            esc(c.status),
            esc(c.label),
            esc(&c.detail)
        ));
    }

    let w = warnings(s);
    let findings = if w.is_empty() {
        "<p class=\"none\">Tidak ada aturan risiko yang terpicu.</p>".to_string()
    } else {
        let mut ul = String::from("<ol>");
        for item in &w {
            ul.push_str(&format!("<li>{}</li>", esc(item)));
        }
        ul.push_str("</ol>");
        ul
    };

    let l2 = &s.l2;

    format!(
        r#"<!DOCTYPE html>
<html lang="id">
<head>
<meta charset="utf-8">
<title>Laporan Pemindaian QRIS - Anti Timpa</title>
<style>
  :root {{ color-scheme: light; }}
  body {{ font-family: system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
         margin: 0; padding: 28px; color: #10151b; background: #f6f7f9; }}
  .sheet {{ max-width: 760px; margin: 0 auto; background: #fff; border: 1px solid #d8dee6;
            border-radius: 10px; padding: 28px; }}
  h1 {{ font-size: 19px; margin: 0 0 4px; }}
  .sub {{ color: #5b6672; font-size: 12px; margin-bottom: 20px; }}
  .band {{ display: inline-block; padding: 6px 14px; border-radius: 999px;
           font-weight: 700; font-size: 14px; color: #fff; }}
  .band.low {{ background: #2e7d32; }}
  .band.caution {{ background: #ef8c00; }}
  .band.high {{ background: #c62828; }}
  .score {{ font-size: 30px; font-weight: 700; margin-left: 12px; vertical-align: middle; }}
  table {{ width: 100%; border-collapse: collapse; margin: 10px 0 18px; font-size: 13px; }}
  th, td {{ text-align: left; padding: 7px 9px; border-bottom: 1px solid #e6eaef;
            vertical-align: top; }}
  th {{ color: #5b6672; font-weight: 600; font-size: 11px; text-transform: uppercase;
        letter-spacing: .05em; }}
  .ran {{ color: #2e7d32; font-weight: 700; }}
  .notrun {{ color: #8a94a0; font-weight: 700; }}
  h2 {{ font-size: 13px; text-transform: uppercase; letter-spacing: .05em;
        color: #5b6672; margin: 22px 0 6px; }}
  ol {{ font-size: 13px; margin: 0; padding-left: 20px; }}
  .none {{ font-size: 13px; color: #5b6672; margin: 0; }}
  code {{ display: block; background: #f1f4f8; border: 1px solid #e1e7ee; border-radius: 6px;
          padding: 10px; font-size: 11px; word-break: break-all; }}
  .meta {{ font-size: 12px; color: #5b6672; }}
  .note {{ font-size: 12px; color: #5b6672; border-top: 1px solid #e6eaef;
           margin-top: 22px; padding-top: 14px; }}
</style>
</head>
<body>
<div class="sheet">
  <h1>Laporan Pemindaian QRIS</h1>
  <div class="sub">Anti Timpa QRIS Scanner &middot; {app_version} &middot; dibuat di perangkat, tanpa jaringan</div>

  <div>
    <span class="band {band_class}">{band}</span>
    <span class="score">{score:.2}</span>
  </div>

  <h2>Metadata</h2>
  <table>
    <tr><th>Waktu</th><td>{ts}</td></tr>
    <tr><th>Sumber</th><td>{source}</td></tr>
    <tr><th>Hash rantai</th><td>{chain}</td></tr>
  </table>

  <h2>Cakupan pemeriksaan</h2>
  <table>
    <tr><th>Status</th><th>Pemeriksaan</th><th>Detail</th></tr>
    {cov_rows}
  </table>

  <h2>Rincian payload (EMVCo TLV)</h2>
  <table>
    <tr><th>Merchant</th><td>{merchant}</td></tr>
    <tr><th>Kota merchant</th><td>{mcity}</td></tr>
    <tr><th>Kota klien</th><td>{ccity}</td></tr>
    <tr><th>MCC</th><td>{mcc}</td></tr>
    <tr><th>Mode inisiasi</th><td>{mode}</td></tr>
    <tr><th>CRC-16</th><td>{crc}</td></tr>
    <tr><th>Ketajaman (Laplacian)</th><td>{blur}</td></tr>
    <tr><th>Skor per lapisan</th><td>L1 {l1:.2} &middot; L2 {l2s:.2} &middot; L3 {l3:.2}</td></tr>
  </table>

  <h2>Temuan</h2>
  {findings}

  <h2>Payload</h2>
  <code>{payload}</code>

  <p class="note">
    Laporan ini dihasilkan di perangkat, tanpa koneksi jaringan. Ketiga lapisan menganalisis
    <strong>payload</strong> dan <strong>gambar</strong>, bukan identitas pemilik QRIS.
    Hasil &ldquo;LOW RISK&rdquo; berarti tidak ada anomali yang terdeteksi, bukan jaminan bahwa QRIS
    tersebut sah. Verifikasi ke merchant sebelum bertransaksi.
  </p>
</div>
</body>
</html>
"#,
        app_version = esc(input.app_version),
        band_class = band_class,
        band = esc(&s.combined_risk_level),
        score = s.combined_score,
        ts = esc(&timestamp_line(input.timestamp_ms)),
        source = esc(input.source),
        chain = match input.chain_hash {
            Some(h) => format!("{h:#018x}"),
            None => "—".to_string(),
        },
        cov_rows = cov_rows,
        merchant = esc(str_field(l2, "merchant_name")),
        mcity = esc(str_field(l2, "merchant_city")),
        ccity = esc(str_field(&s.l3, "client_city")),
        mcc = esc(str_field(l2, "mcc")),
        mode = esc(str_field(l2, "initiation_mode")),
        crc = if bool_field(l2, "crc_valid") { "VALID" } else { "GAGAL" },
        blur = if s.blur_var > 0.0 {
            format!("{:.1}", s.blur_var)
        } else {
            "—".to_string()
        },
        l1 = f_field(&s.l1, "l1_score"),
        l2s = f_field(&s.l2, "l2_score"),
        l3 = f_field(&s.l3, "l3_score"),
        payload = esc(&s.raw_qris_str),
        findings = findings,
    )
}

pub fn render(input: &ReportInput, format: ReportFormat) -> String {
    match format {
        ReportFormat::Text => render_text(input),
        ReportFormat::Html => render_html(input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot_with(payload_merchant: &str, level: &str, l1: serde_json::Value) -> ScanSnapshot {
        ScanSnapshot {
            l1,
            l2: json!({
                "l2_score": 0.5,
                "crc_valid": true,
                "initiation_mode": "11",
                "mcc": "5411",
                "merchant_name": payload_merchant,
                "merchant_city": "JAKARTA",
                "parsed_tlv": {},
                "warnings": ["contoh peringatan"],
            }),
            l3: json!({
                "l3_score": 0.0,
                "risk_level": "LOW RISK",
                "warnings": [],
                "client_city": "JAKARTA",
                "merchant_city": "JAKARTA",
            }),
            combined_score: 0.5,
            combined_risk_level: level.into(),
            is_blurry: false,
            blur_var: 120.0,
            qr_bbox: None,
            raw_qris_str: "000201010211".into(),
            no_qr_reason: None,
            coverage: crate::ScanCoverage {
                optical_ran: true,
                payload_ran: true,
                geofence_ran: true,
                complete: true,
                summary: "Pemeriksaan lengkap.".into(),
            },
            findings: Vec::new(),
            chain_hash: None,
        }
    }

    fn ran_l1() -> serde_json::Value {
        json!({
            "l1_score": 0.8,
            "spatial_edge_density": 0.21,
            "temporal_glare_var": 0.004,
            "risk_level": "HIGH RISK",
        })
    }

    fn not_run_l1() -> serde_json::Value {
        json!({
            "l1_score": 0.0,
            "spatial_edge_density": 0.0,
            "temporal_glare_var": 0.0,
            "risk_level": "NOT RUN",
        })
    }

    #[test]
    fn text_report_states_that_layer1_did_not_run() {
        // The single most important property of a report: it must not imply
        // coverage it does not have.
        let snap = snapshot_with("WARUNG", "CAUTION", not_run_l1());
        let text = render_text(&ReportInput {
            snapshot: &snap,
            source: "manual",
            timestamp_ms: Some(1),
            app_version: "0.1.0",
            chain_hash: None,
        });
        assert!(text.contains("[NOT RUN]"), "got: {text}");
        assert!(text.contains("no frame was available"));
    }

    #[test]
    fn text_report_marks_layer1_as_run_when_scored() {
        let snap = snapshot_with("WARUNG", "HIGH RISK", ran_l1());
        let text = render_text(&ReportInput {
            snapshot: &snap,
            source: "camera",
            timestamp_ms: Some(1),
            app_version: "0.1.0",
            chain_hash: Some(0xabcd),
        });
        assert!(text.contains("[RAN]"));
        assert!(text.contains("edge_density=0.2100"));
        assert!(text.contains("0x000000000000abcd"));
    }

    #[test]
    fn html_report_escapes_attacker_controlled_merchant_name() {
        // A merchant name comes from the QR payload, i.e. from an attacker.
        // If it is not escaped, the report becomes an injection vector.
        let snap = snapshot_with("<script>alert(1)</script>", "HIGH RISK", ran_l1());
        let html = render_html(&ReportInput {
            snapshot: &snap,
            source: "manual",
            timestamp_ms: Some(1),
            app_version: "0.1.0",
            chain_hash: None,
        });
        assert!(!html.contains("<script>alert(1)</script>"), "must be escaped");
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    }

    #[test]
    fn html_report_is_self_contained() {
        let snap = snapshot_with("WARUNG", "LOW RISK", ran_l1());
        let html = render_html(&ReportInput {
            snapshot: &snap,
            source: "manual",
            timestamp_ms: Some(1),
            app_version: "0.1.0",
            chain_hash: None,
        });
        // No external references: the document must render offline.
        assert!(!html.contains("http://"), "no external http references");
        assert!(!html.contains("https://"), "no external https references");
        assert!(html.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn html_uses_the_right_band_colour() {
        let high = snapshot_with("W", "HIGH RISK", ran_l1());
        let html = render_html(&ReportInput {
            snapshot: &high,
            source: "manual",
            timestamp_ms: None,
            app_version: "0.1.0",
            chain_hash: None,
        });
        assert!(html.contains("band high"));
        assert!(html.contains("unavailable (platform clock not supplied)"));
    }

    #[test]
    fn format_parsing_defaults_to_text() {
        assert_eq!(ReportFormat::parse("html"), ReportFormat::Html);
        assert_eq!(ReportFormat::parse("HTML"), ReportFormat::Html);
        assert_eq!(ReportFormat::parse("pdf"), ReportFormat::Text);
    }
}
