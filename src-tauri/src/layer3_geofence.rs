//! Layer 3 — City geofence.
//!
//! Direct port of `layer3_geofence.py::process_layer3_geofence`.
//! The rule set is intentionally identical so results stay byte-for-byte
//! comparable with the existing Python implementation.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeofenceResult {
    pub l3_score: f64,
    pub risk_level: String,
    pub warnings: Vec<String>,
    pub client_city: Option<String>,
    pub merchant_city: Option<String>,
}

fn low(client_city: Option<String>, merchant_city: Option<String>, warning: &str) -> GeofenceResult {
    GeofenceResult {
        l3_score: 0.0,
        risk_level: "LOW RISK".to_string(),
        warnings: vec![warning.to_string()],
        client_city,
        merchant_city,
    }
}

/// Compares the client city (from GPS reverse geocoding) against the merchant
/// city carried in EMVCo Tag 60. A mismatch is a hard geofence anomaly.
pub fn process_layer3_geofence(
    client_city: Option<&str>,
    merchant_city: Option<&str>,
) -> GeofenceResult {
    let merchant_city = merchant_city.filter(|c| !c.is_empty());
    let client_city = client_city.filter(|c| !c.is_empty());

    let merchant_city = match merchant_city {
        Some(c) => c,
        None => {
            return low(
                client_city.map(str::to_string),
                None,
                "Merchant City missing from QR code - skipped geofence check",
            )
        }
    };

    let client_city = match client_city {
        Some(c) => c,
        None => {
            return low(
                None,
                Some(merchant_city.to_string()),
                "Client Location unavailable - skipped geofence check",
            )
        }
    };

    if client_city == "LOADING" {
        return GeofenceResult {
            l3_score: 0.5,
            risk_level: "CAUTION".to_string(),
            warnings: vec!["Client Location is loading (API call in progress)...".to_string()],
            client_city: Some(client_city.to_string()),
            merchant_city: Some(merchant_city.to_string()),
        };
    }

    // Simplify common Indonesian administrative prefixes for robust matching.
    const REMOVALS: [&str; 3] = ["KOTA ", "KABUPATEN ", "KAB. "];

    let normalize = |input: &str| -> String {
        let mut out = input.to_uppercase();
        for prefix in REMOVALS {
            out = out.replace(prefix, "");
        }
        out.trim().to_string()
    };

    let c_city = normalize(client_city);
    let m_city = normalize(merchant_city);

    // Exact match, or substring match (e.g. "JAKARTA" in "JAKARTA SELATAN").
    let matches = c_city == m_city || c_city.contains(&m_city) || m_city.contains(&c_city);

    if matches {
        GeofenceResult {
            l3_score: 0.0,
            risk_level: "LOW RISK".to_string(),
            warnings: Vec::new(),
            client_city: Some(client_city.to_string()),
            merchant_city: Some(merchant_city.to_string()),
        }
    } else {
        GeofenceResult {
            l3_score: 1.0,
            risk_level: "HIGH RISK".to_string(),
            warnings: vec![format!(
                "Geofence Anomaly: Client is in '{}', but QR is for '{}'",
                client_city, merchant_city
            )],
            client_city: Some(client_city.to_string()),
            merchant_city: Some(merchant_city.to_string()),
        }
    }
}
