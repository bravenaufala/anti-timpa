//! Layer 2: EMVCo MPM payload validation.
//!
//! Tag-Length-Value parsing and rule-based risk scoring:
//!   * `parse_emvco_tlv`       -> Tag-Length-Value parser (flat + nested)
//!   * `verify_crc16`          -> CRC-16/CCITT-FALSE
//!   * `process_layer2_tlv`    -> rule-based risk scoring
//!
//! The parsing and scoring rules follow the EMVCo MPM specification, so the
//! existing fixtures remain the spec.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer2Result {
    pub l2_score: f64,
    pub crc_valid: bool,
    pub initiation_mode: String,
    pub mcc: String,
    pub merchant_name: String,
    pub merchant_city: String,
    /// National Merchant ID (NMID) or equivalent, read from the nested merchant
    /// account sub-TLVs (tags 26..=51).
    ///
    /// This is surfaced for *reporting*, not verification: the identifier is the
    /// cross-check key a bank or PSP looks up server-side. Anti Timpa has no
    /// reference database, so it shows the value rather than asserting the
    /// account behind it is legitimate.
    pub merchant_id: String,
    pub parsed_tlv: Value,
    pub warnings: Vec<String>,
}

/// Result of TLV parsing: either a tag map or a parse failure.
pub fn parse_emvco_tlv(raw: &str) -> Result<Map<String, Value>, String> {
    if raw.is_empty() {
        return Err("Empty or non-string QRIS payload".to_string());
    }

    fn parse_blocks(s: &str) -> Result<Map<String, Value>, ()> {
        let bytes = s.as_bytes();
        let n = bytes.len();
        if n == 0 {
            return Err(());
        }

        let mut out: Map<String, Value> = Map::new();
        let mut idx = 0usize;

        while idx < n {
            // Need at least 4 bytes for Tag (2) + Length (2).
            if idx + 4 > n {
                return Err(());
            }

            let tag = &s[idx..idx + 2];
            let len_str = &s[idx + 2..idx + 4];

            // Length must be exactly two ASCII digits.
            if !len_str.bytes().all(|b| b.is_ascii_digit()) {
                return Err(());
            }
            let length: usize = len_str.parse().map_err(|_| ())?;
            idx += 4;

            if idx + length > n {
                return Err(());
            }

            // Guard against slicing inside a multi-byte UTF-8 codepoint.
            if !s.is_char_boundary(idx) || !s.is_char_boundary(idx + length) {
                return Err(());
            }
            let value = &s[idx..idx + length];
            idx += length;

            // Merchant account tags (26..=51) may carry nested sub-TLVs.
            let tag_as_num = tag.parse::<u32>().ok();
            match tag_as_num {
                Some(num) if (26..=51).contains(&num) => match parse_blocks(value) {
                    Ok(sub) if !sub.is_empty() => {
                        out.insert(tag.to_string(), Value::Object(sub));
                    }
                    _ => {
                        out.insert(tag.to_string(), Value::String(value.to_string()));
                    }
                },
                _ => {
                    out.insert(tag.to_string(), Value::String(value.to_string()));
                }
            }
        }

        if idx != n {
            return Err(());
        }
        Ok(out)
    }

    match parse_blocks(raw) {
        Ok(map) => Ok(map),
        Err(()) => Err("Structural TLV parsing failure".to_string()),
    }
}

/// CRC-16/CCITT-FALSE over the payload up to and including the `6304` tag.
pub fn verify_crc16(raw: &str) -> bool {
    let Some(crc_pos) = raw.rfind("6304") else {
        return false;
    };
    if raw.len() < crc_pos + 8 {
        return false;
    }

    let payload_to_check = &raw[..crc_pos + 4];
    let expected_crc = &raw[crc_pos + 4..crc_pos + 8];

    if expected_crc.len() != 4 {
        return false;
    }

    let mut crc: u16 = 0xFFFF;
    for byte in payload_to_check.as_bytes() {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }

    let calculated = format!("{crc:04X}");
    calculated.eq_ignore_ascii_case(expected_crc)
}

/// Reads the merchant account identifier from the nested merchant account
/// sub-TLVs (tags 26..=51).
///
/// EMVCo carries the national merchant id as a sub-tag of one of these tags;
/// Indonesian QRIS conventionally puts the NMID under sub-tag `01` or `02`.
/// Returns an empty string when no such sub-tag exists, so the caller can tell
/// "no account id in the payload" from a value.
fn merchant_account_id(tlv: &Map<String, Value>) -> String {
    for tag in 26..=51u32 {
        let key = format!("{tag:02}");
        if let Some(Value::Object(sub)) = tlv.get(&key) {
            for sub_tag in ["02", "01"] {
                if let Some(Value::String(v)) = sub.get(sub_tag) {
                    if !v.trim().is_empty() {
                        return v.clone();
                    }
                }
            }
        }
    }
    String::new()
}

/// Convenience accessor: the tag's value as a string, empty when absent.
fn tag_str(tlv: &Map<String, Value>, tag: &str) -> String {
    match tlv.get(tag) {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// Full Layer 2 evaluation: structural validity, CRC, and risk rules.
pub fn process_layer2_tlv(raw: &str, optical_type: Option<&str>) -> Layer2Result {
    let mut warnings: Vec<String> = Vec::new();
    let mut accumulated_risk = 0.0f64;

    let parsed = parse_emvco_tlv(raw);
    let crc_valid = verify_crc16(raw);

    let parsed_tlv: Value = match &parsed {
        Ok(map) => Value::Object(map.clone()),
        Err(msg) => serde_json::json!({ "valid": false, "error": msg }),
    };

    let valid = parsed.is_ok();

    // 1. Structural TLV check.
    if !valid {
        warnings.push("Invalid TLV payload structure".to_string());
    }

    // 2. Checksum failure (Tag 63 missing, or CRC mismatch).
    let has_tag63 = match &parsed {
        Ok(map) => map.contains_key("63"),
        Err(_) => false,
    };
    if !crc_valid || !has_tag63 {
        warnings.push("CRC-16 checksum verification failed or Tag 63 missing".to_string());
        accumulated_risk = 1.0;
    }

    let (initiation_mode, mcc, merchant_name, merchant_city) = match &parsed {
        Ok(map) => (
            tag_str(map, "01"),
            tag_str(map, "52"),
            tag_str(map, "59"),
            tag_str(map, "60"),
        ),
        Err(_) => (String::new(), String::new(), String::new(), String::new()),
    };
    let payload_format = parsed.as_ref().map(|m| tag_str(m, "00")).unwrap_or_default();
    let currency = parsed.as_ref().map(|m| tag_str(m, "53")).unwrap_or_default();
    let country = parsed.as_ref().map(|m| tag_str(m, "58")).unwrap_or_default();
    let merchant_id = parsed
        .as_ref()
        .map(merchant_account_id)
        .unwrap_or_default();

    // Fine-grained rules only apply to structurally valid, checksum-clean payloads.
    if crc_valid && valid {
        // Rule A: payload format indicator must be "01".
        if payload_format != "01" {
            accumulated_risk += 0.50;
            warnings.push(
                "Payload Format Indicator (Tag 00) is invalid or not '01'".to_string(),
            );
        }

        // Rule B: currency must be IDR (360) and country ID.
        if currency != "360" || country != "ID" {
            accumulated_risk += 0.30;
            warnings.push(
                "Transaction currency (Tag 53) or country code (Tag 58) anomaly".to_string(),
            );
        }

        // Rule C: dynamic QR (Tag 01 = "12") in a physical camera scan context.
        if initiation_mode == "12" && optical_type == Some("physical_camera_scan") {
            accumulated_risk += 0.40;
            warnings.push(
                "Dynamic QR code (Tag 01=12) scanned in physical camera scan context".to_string(),
            );
        }

        // Rule D: charity MCC paired with a commercial-looking merchant name.
        const CHARITY_MCCS: [&str; 2] = ["8661", "8398"];
        const COMMERCIAL_KEYWORDS: [&str; 7] =
            ["toko", "store", "cell", "mart", "warung", "cafe", "kopi"];
        let mname = merchant_name.to_lowercase();

        if CHARITY_MCCS.contains(&mcc.as_str())
            && COMMERCIAL_KEYWORDS.iter().any(|kw| mname.contains(kw))
        {
            accumulated_risk += 0.50;
            warnings.push(format!(
                "MCC misrepresentation anomaly: Charity MCC ({mcc}) paired with commercial merchant name ('{merchant_name}')"
            ));
        }
    }

    Layer2Result {
        l2_score: accumulated_risk.clamp(0.0, 1.0),
        crc_valid,
        initiation_mode,
        mcc,
        merchant_name,
        merchant_city,
        merchant_id,
        parsed_tlv,
        warnings,
    }
}
