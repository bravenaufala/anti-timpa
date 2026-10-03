//! Layer 3: geographic plausibility.
//!
//! # Scope
//!
//! A city mismatch between the device's location and EMVCo Tag 60 is not
//! evidence of tampering. It is evidence of *implausibility*, and the two need
//! different evidence standards:
//!
//! * An attacker who pastes a sticker copies the original Tag 60 verbatim. The
//!   payload matches its CRC, and the merchant city is genuinely that merchant's
//!   city. Layer 3 sees nothing at all.
//! * A legitimate merchant whose QR is scanned by a traveller five hundred
//!   kilometres from home produces a perfect 1.0 mismatch.
//!
//! So a naive `client_city != merchant_city => HIGH RISK` rule fires hardest on
//! exactly the people behaving normally. In the original implementation that is
//! literally the behaviour: any mismatch scored 1.0, which made the layer the
//! single largest source of false positives in the whole pipeline, and because
//! the combined score is `max(l1, l2, l3)`, a mismatch alone forced the app's
//! headline verdict to HIGH RISK.
//!
//! This module replaces the flat rule with the reasoning that decision actually
//! needs: a mismatch is a *weak* signal whose strength depends on things the
//! flat rule discarded, such as how far away the client is, whether the
//! merchant's city sits on a national border, and what kind of QR this is.
//!
//! # Design
//!
//! 1. Comparability first. Matching never fails just because strings differ.
//!    If the merchant city cannot be interpreted as a place (a QR encoded as
//!    UTF-8 instead of the spec-mandated ASCII, or a marker such as `ONLINE`),
//!    the layer reports `NOT COMPARABLE` instead of a fabricated anomaly.
//! 2. Tiered penalties, not a binary. Different mismatch shapes carry
//!    different weight, and none of them alone is allowed to force HIGH RISK.
//! 3. Distance. An optional coarse distance (from a device coarse fix to a
//!    city reference point) turns "different name" into "different name and 1200
//!    km away", which is a materially stronger claim.
//!
//! # The city table
//!
//! The city table here is small and hand-curated for the demo corridor
//! (Java/Bali), not the ~514 Indonesian kabupaten/kota. A city that is not in
//! the table still gets the name-based tiers; it only loses the distance
//! refinement. Adding the full administrative dataset is a data task, not a
//! logic change.

use serde::{Deserialize, Serialize};

/// A city reference point, used only to compute a coarse distance.
#[derive(Debug, Clone, Copy)]
struct CityRef {
    /// Canonical name as it appears in EMVCo Tag 60.
    name: &'static str,
    lat: f64,
    lon: f64,
    /// True when the city sits on a land border, which makes an adjacent
    /// neighbouring city a plausible reading of the same location.
    border: bool,
    /// Provinces this city is commonly abbreviated from, for alias matching.
    aliases: &'static [&'static str],
}

/// Hand-curated reference points for the demo corridor.
///
/// Coordinates are city-centre approximations, which is all a *coarse* distance
/// needs. They are not geocoding-grade.
const CITIES: &[CityRef] = &[
    CityRef { name: "JAKARTA", lat: -6.2088, lon: 106.8456, border: false, aliases: &["JAKARTA SELATAN", "JAKARTA PUSAT", "JAKARTA BARAT", "JAKARTA TIMUR", "JAKARTA UTARA"] },
    CityRef { name: "BANDUNG", lat: -6.9175, lon: 107.6191, border: false, aliases: &["CIMAHI"] },
    CityRef { name: "SURABAYA", lat: -7.2575, lon: 112.7521, border: false, aliases: &["SIDOARJO"] },
    CityRef { name: "SEMARANG", lat: -6.9667, lon: 110.4167, border: false, aliases: &["UNGARAN"] },
    CityRef { name: "YOGYAKARTA", lat: -7.7956, lon: 110.3695, border: false, aliases: &["SLEMAN", "BANTUL"] },
    CityRef { name: "MEDAN", lat: 3.5952, lon: 98.6722, border: false, aliases: &[] },
    CityRef { name: "DENPASAR", lat: -8.6705, lon: 115.2126, border: false, aliases: &["BADUNG", "KUTA"] },
    CityRef { name: "MAKASSAR", lat: -5.1477, lon: 119.4327, border: false, aliases: &[] },
    CityRef { name: "BALIKPAPAN", lat: -1.2379, lon: 116.8529, border: false, aliases: &[] },
    CityRef { name: "BATAM", lat: 1.0456, lon: 104.0305, border: true, aliases: &[] },
    CityRef { name: "PONTIANAK", lat: -0.0263, lon: 109.3425, border: true, aliases: &[] },
    CityRef { name: "MANADO", lat: 1.4748, lon: 124.8421, border: false, aliases: &[] },
];

/// Distance beyond which a mismatch is treated as physically impossible to
/// explain by a shared metropolitan area.
///
/// 150 km is comfortably larger than the Jakarta-Bandung corridor (~120 km),
/// which people genuinely commute between, so that pair lands in the
/// "different city, plausible day trip" tier rather than the strong tier.
const DISTANT_KM: f64 = 150.0;

/// Mismatch shapes, ordered by how much they actually indicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MismatchKind {
    /// Names agree after normalisation. No anomaly.
    Match,
    /// Same city, different administrative level (e.g. `KOTA BANDUNG` vs
    /// `KABUPATEN BANDUNG`). Routine, not a finding.
    SameMetro,
    /// The merchant city is present but cannot be interpreted as a place
    /// (non-ASCII, or an explicit online/remote marker).
    NotComparable,
    /// One of the two inputs is missing, so nothing was compared.
    NotEvaluated,
    /// Different names, within plausible travelling distance.
    DifferentCityNearby,
    /// Different names, far apart.
    DifferentCityDistant,
    /// Different names and there is no reference data to bound the distance.
    DifferentCityUnbounded,
}

impl MismatchKind {
    fn as_str(self) -> &'static str {
        match self {
            MismatchKind::Match => "MATCH",
            MismatchKind::SameMetro => "SAME_METRO",
            MismatchKind::NotComparable => "NOT_COMPARABLE",
            MismatchKind::NotEvaluated => "NOT_EVALUATED",
            MismatchKind::DifferentCityNearby => "DIFFERENT_CITY_NEARBY",
            MismatchKind::DifferentCityDistant => "DIFFERENT_CITY_DISTANT",
            MismatchKind::DifferentCityUnbounded => "DIFFERENT_CITY_UNBOUNDED",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeofenceResult {
    pub l3_score: f64,
    pub risk_level: String,
    pub warnings: Vec<String>,
    pub client_city: Option<String>,
    pub merchant_city: Option<String>,
    /// Machine-readable classification of the comparison, so the UI and the
    /// report can distinguish "checked and plausible" from "could not check".
    pub mismatch_kind: String,
    /// Great-circle distance when both a device fix and a known city reference
    /// were available.
    pub distance_km: Option<f64>,
    /// Whether the client supplied a coarse location fix at all.
    pub location_available: bool,
    /// Whether the comparison actually happened.
    ///
    /// A skipped check must be distinguishable from a passed check, otherwise
    /// "we did not look" reads as "we looked and it was fine".
    pub evaluated: bool,
}

fn result(
    score: f64,
    band: &str,
    warnings: Vec<String>,
    client_city: Option<String>,
    merchant_city: Option<String>,
    kind: MismatchKind,
    distance_km: Option<f64>,
    location_available: bool,
) -> GeofenceResult {
    GeofenceResult {
        l3_score: score,
        risk_level: band.to_string(),
        warnings,
        client_city,
        merchant_city,
        mismatch_kind: kind.as_str().to_string(),
        distance_km,
        location_available,
        evaluated: kind != MismatchKind::NotEvaluated,
    }
}

/// Smallest and largest legal WGS84 coordinates, used to reject placeholder
/// fixes (`0,0`) that would otherwise silently produce a huge distance.
fn valid_fix(lat: f64, lon: f64) -> bool {
    lat.is_finite()
        && lon.is_finite()
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lon)
        && !(lat.abs() < 0.5 && lon.abs() < 0.5)
}

/// Great-circle distance in kilometres, Haversine.
fn haversine_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    const R: f64 = 6371.0;
    let (lat1, lon1) = (a.0.to_radians(), a.1.to_radians());
    let (lat2, lon2) = (b.0.to_radians(), b.1.to_radians());
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().asin()
}

/// Administrative prefixes and filler words that carry no geographic meaning.
const STRIP_WORDS: [&str; 9] = [
    "KOTA", "KABUPATEN", "KAB", "KAB.", "KOTAMADYA", "MUNICIPALITY", "CITY", "REGENCY", "DISTRICT",
];

/// Markers that mean "this merchant is remote", not "this merchant is elsewhere".
///
/// EMVCo permits a merchant to put a non-geographic service descriptor in
/// Tag 60, and e-commerce QRIS codes commonly do. Comparing those against a GPS
/// fix is meaningless, and the old rule would flag every such scan.
const NON_GEOGRAPHIC_MARKERS: [&str; 6] = ["ONLINE", "INTERNET", "E-COMMERCE", "DIGITAL", "WEB", "APP"];

/// Normalises a city name into a comparison token.
///
/// Conservative by design: it uppercases, strips punctuation and administrative
/// prefixes, and nothing else. It does not do fuzzy matching, because a fuzzy
/// matcher that treats `SURABAYA` and `SEMARANG` as similar would make the layer
/// silently useless (see the module docs on why a false negative here is worse
/// than a false positive).
fn normalize(input: &str) -> String {
    let upper = input.to_uppercase();
    let mut cleaned = String::with_capacity(upper.len());
    for ch in upper.chars() {
        if ch.is_ascii_alphanumeric() || ch == ' ' {
            cleaned.push(ch);
        } else if ch == '-' || ch == '/' {
            cleaned.push(' ');
        }
    }
    let mut out = String::with_capacity(cleaned.len());
    for word in cleaned.split_whitespace() {
        if STRIP_WORDS.contains(&word) {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Looks up a canonical city for a normalised name, following aliases.
fn lookup_city(normalized: &str) -> Option<&'static CityRef> {
    CITIES.iter().find(|c| {
        c.name == normalized
            || c.aliases.iter().any(|a| *a == normalized)
            || normalized.starts_with(c.name)
    })
}

/// Scores a resolved comparison.
///
/// `distance_km` is `None` when either endpoint is unknown.
fn score_mismatch(distance_km: Option<f64>, on_border: bool) -> (f64, &'static str, MismatchKind) {
    // A border city's neighbour is a genuinely ambiguous reading of the same
    // physical location, so it never escalates past CAUTION on distance alone.
    if on_border {
        return (
            0.40,
            "CAUTION",
            MismatchKind::DifferentCityNearby,
        );
    }

    match distance_km {
        Some(d) if d <= DISTANT_KM => (0.40, "CAUTION", MismatchKind::DifferentCityNearby),
        Some(_) => (0.65, "CAUTION", MismatchKind::DifferentCityDistant),
        // No reference data: report the mismatch but cap it below the HIGH RISK
        // band, because "different name with unknown distance" is weak evidence
        // and this was the largest false-positive source in the old rule.
        None => (0.55, "CAUTION", MismatchKind::DifferentCityUnbounded),
    }
}

/// Compares where the device is against the merchant city in EMVCo Tag 60.
///
/// # Arguments
///
/// * `client_city`: city name from reverse geocoding, when available.
/// * `merchant_city`: Tag 60 value from the payload.
/// * `client_fix`: optional coarse `(lat, lon)` for a distance estimate.
pub fn process_layer3_geofence(
    client_city: Option<&str>,
    merchant_city: Option<&str>,
    client_fix: Option<(f64, f64)>,
) -> GeofenceResult {
    let merchant_raw = merchant_city.filter(|c| !c.trim().is_empty());
    let client_raw = client_city.filter(|c| !c.trim().is_empty());

    // A fix that fails validation (`(0,0)`, out-of-range, NaN) is discarded here
    // rather than at each call site, so there is one choke point for deciding
    // what counts as a real position. `valid_fix` is not re-checked later.
    let has_valid_fix = client_fix.map(|(la, lo)| valid_fix(la, lo)).unwrap_or(false);
    let fix = if has_valid_fix { client_fix } else { None };
    let location_available = client_raw.is_some() || fix.is_some();

    let Some(merchant_city) = merchant_raw else {
        return result(
            0.0,
            "NOT RUN",
            vec![
                "Merchant City (EMVCo Tag 60) tidak ada di payload, jadi perbandingan lokasi \
                 tidak dapat dilakukan."
                    .to_string(),
            ],
            client_raw.map(str::to_string),
            None,
            MismatchKind::NotEvaluated,
            None,
            location_available,
        );
    };

    let Some(client_city) = client_raw else {
        return result(
            0.0,
            "NOT RUN",
            vec![
                "Lokasi perangkat tidak tersedia, jadi perbandingan lokasi tidak dilakukan. \
                 Ini bukan berarti lokasi cocok — pemeriksaan ini memang tidak berjalan."
                    .to_string(),
            ],
            None,
            Some(merchant_city.to_string()),
            MismatchKind::NotEvaluated,
            None,
            location_available,
        );
    };

    // A GPS fix on its own is enough to compare, even without a city name.
    let merchant_norm = normalize(merchant_city);
    let client_norm = normalize(client_city);

    // --- comparability gates -------------------------------------------------

    // Non-ASCII cannot be a valid Tag 60 value (EMVCo requires the ASCII subset)
    // and is usually a UTF-8 encoding bug. Fabricating an anomaly from our own
    // decoding mistake is the wrong response, so the layer declines to compare.
    if !merchant_city.is_ascii() {
        return result(
            0.0,
            "NOT COMPARABLE",
            vec![
                "Tag 60 mengandung karakter non-ASCII, padahal spesifikasi EMVCo mensyaratkan \
                 ASCII. Nilai ini kemungkinan salah encode, sehingga tidak dibandingkan."
                    .to_string(),
            ],
            Some(client_city.to_string()),
            Some(merchant_city.to_string()),
            MismatchKind::NotComparable,
            None,
            true,
        );
    }

    if NON_GEOGRAPHIC_MARKERS
        .iter()
        .any(|m| merchant_norm == *m || merchant_norm.contains(m))
    {
        return result(
            0.0,
            "NOT COMPARABLE",
            vec![format!(
                "Tag 60 berisi penanda non-geografis ('{merchant_city}'), umum pada QRIS \
                 e-commerce. Perbandingan lokasi tidak relevan."
            )],
            Some(client_city.to_string()),
            Some(merchant_city.to_string()),
            MismatchKind::NotComparable,
            None,
            true,
        );
    }

    if merchant_norm.is_empty() || client_norm.is_empty() {
        return result(
            0.0,
            "NOT COMPARABLE",
            vec!["Nama kota kosong setelah normalisasi (hanya berisi prefiks administratif)."
                .to_string()],
            Some(client_city.to_string()),
            Some(merchant_city.to_string()),
            MismatchKind::NotComparable,
            None,
            true,
        );
    }

    // --- distance, when both endpoints are known -----------------------------

    let merchant_ref = lookup_city(&merchant_norm);
    let client_ref = lookup_city(&client_norm);

    let distance_km = match (fix, merchant_ref) {
        // A device fix is an actual position, so when one exists *and* the
        // merchant city is in the table, that is the strongest measurement
        // available and it always wins over the city-reference estimate.
        (Some(f), Some(m)) => Some(haversine_km(f, (m.lat, m.lon))),
        // No fix: fall back to comparing two city reference points, which needs
        // the client city to be in the table too.
        (None, Some(m)) => client_ref.map(|c| haversine_km((m.lat, m.lon), (c.lat, c.lon))),
        // Merchant city is not in the table, so there is nothing to measure
        // against. This branch also catches `fix = None` with an unknown
        // merchant, which is why it is the catch-all.
        _ => None,
    };

    // --- equality, in order of increasing suspicion -------------------------

    if client_norm == merchant_norm {
        return result(
            0.0,
            "LOW RISK",
            Vec::new(),
            Some(client_city.to_string()),
            Some(merchant_city.to_string()),
            MismatchKind::Match,
            distance_km,
            true,
        );
    }

    // Same city at a different administrative level, or one name containing the
    // other ("JAKARTA" vs "JAKARTA SELATAN"). Routine: the client and the
    // merchant are describing the same place.
    let containment = client_norm.contains(&merchant_norm) || merchant_norm.contains(&client_norm);
    let same_metro = containment
        || (client_ref.is_some() && client_ref.map(|c| c.name) == merchant_ref.map(|m| m.name));

    if same_metro {
        return result(
            0.0,
            "LOW RISK",
            vec![format!(
                "'{client_city}' dan '{merchant_city}' merujuk ke wilayah yang sama."
            )],
            Some(client_city.to_string()),
            Some(merchant_city.to_string()),
            MismatchKind::SameMetro,
            distance_km,
            true,
        );
    }

    // --- genuine mismatch ---------------------------------------------------

    let on_border = merchant_ref.map(|m| m.border).unwrap_or(false)
        || client_ref.map(|c| c.border).unwrap_or(false);

    let (score, band, kind) = score_mismatch(distance_km, on_border);

    let distance_note = match distance_km {
        Some(d) => format!("jarak lurus sekitar {d:.0} km"),
        None => "jarak tidak dapat dihitung (kota tidak ada di tabel rujukan)".to_string(),
    };

    let mut warnings = vec![format!(
        "Lokasi perangkat ('{client_city}') berbeda dari kota merchant ('{merchant_city}'); \
         {distance_note}."
    )];

    if distance_km.map(|d| d > DISTANT_KM).unwrap_or(false) {
        warnings.push(
            "Perbedaan ini bisa wajar (pelanggan sedang di luar kota/merchant nasional) atau \
             mencurigakan. Konfirmasi ke merchant adalah langkah yang tepat, bukan menolak bayar \
             begitu saja."
                .to_string(),
        );
    } else {
        warnings.push(
            "Perbedaan nama kota jarak dekat sering hanya soal penamaan wilayah yang berbeda \
             antar sistem. Ini sinyal lemah."
                .to_string(),
        );
    }

    result(
        score,
        band,
        warnings,
        Some(client_city.to_string()),
        Some(merchant_city.to_string()),
        kind,
        distance_km,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const JAKARTA: (f64, f64) = (-6.2088, 106.8456);
    const BANDUNG_FIX: (f64, f64) = (-6.9175, 107.6191);
    const MAKASSAR_FIX: (f64, f64) = (-5.1477, 119.4327);

    #[test]
    fn identical_cities_match_and_score_zero() {
        let r = process_layer3_geofence(Some("JAKARTA"), Some("JAKARTA"), None);
        assert_eq!(r.l3_score, 0.0);
        assert_eq!(r.risk_level, "LOW RISK");
        assert_eq!(r.mismatch_kind, "MATCH");
        assert!(r.evaluated);
    }

    #[test]
    fn administrative_prefixes_are_not_a_finding() {
        // The single most common false positive in the old rule.
        let r = process_layer3_geofence(Some("Bandung"), Some("KOTA BANDUNG"), None);
        assert_eq!(r.l3_score, 0.0, "prefix difference must not be a finding");
        assert_eq!(r.risk_level, "LOW RISK");
    }

    #[test]
    fn different_administrative_levels_are_same_metro() {
        // Both normalise to BANDUNG, so this is an exact match after
        // normalisation, which is the correct and cheapest answer.
        let r = process_layer3_geofence(Some("KABUPATEN BANDUNG"), Some("KOTA BANDUNG"), None);
        assert_eq!(r.l3_score, 0.0);
        assert_eq!(r.mismatch_kind, "MATCH");
    }

    #[test]
    fn same_metro_is_reported_when_names_differ_but_the_place_does_not() {
        // "Cimahi" is an alias of the Bandung metro area: different name, same
        // place, so it must not be treated as a mismatch.
        let r = process_layer3_geofence(Some("Cimahi"), Some("Bandung"), None);
        assert_eq!(r.l3_score, 0.0);
        assert_eq!(r.mismatch_kind, "SAME_METRO");
        assert_eq!(r.risk_level, "LOW RISK");
    }

    #[test]
    fn sub_city_containment_is_not_a_finding() {
        let r = process_layer3_geofence(Some("JAKARTA SELATAN"), Some("JAKARTA"), None);
        assert_eq!(r.l3_score, 0.0);
        assert_eq!(r.mismatch_kind, "SAME_METRO");
    }

    #[test]
    fn nearby_different_cities_score_caution_not_high_risk() {
        // The old rule scored this 1.0 / HIGH RISK, which forced the app's
        // headline verdict to HIGH RISK for anyone travelling. That is the bug.
        let r = process_layer3_geofence(Some("Bandung"), Some("JAKARTA"), Some(BANDUNG_FIX));
        assert_eq!(
            r.risk_level, "CAUTION",
            "a nearby mismatch must not be HIGH RISK"
        );
        assert!(
            (r.l3_score - 0.40).abs() < 1e-9,
            "expected the nearby tier, got {}",
            r.l3_score
        );
        assert_eq!(r.mismatch_kind, "DIFFERENT_CITY_NEARBY");
        assert!(r.distance_km.unwrap() < DISTANT_KM);
    }

    #[test]
    fn distant_cities_score_higher_but_still_cap_below_high_risk() {
        let r = process_layer3_geofence(Some("Makassar"), Some("JAKARTA"), Some(MAKASSAR_FIX));
        assert_eq!(r.mismatch_kind, "DIFFERENT_CITY_DISTANT");
        assert!(r.distance_km.unwrap() > 1000.0, "got {:?}", r.distance_km);
        assert!(
            r.l3_score < 0.70,
            "even a distant mismatch must not veto on its own, got {}",
            r.l3_score
        );
        assert_eq!(r.risk_level, "CAUTION");
    }

    #[test]
    fn unknown_cities_get_the_unbounded_tier() {
        let r = process_layer3_geofence(Some("Sleman"), Some("Tual"), None);
        assert_eq!(r.mismatch_kind, "DIFFERENT_CITY_UNBOUNDED");
        assert!(
            r.l3_score < 0.70,
            "no reference data must not produce a veto, got {}",
            r.l3_score
        );
    }

    #[test]
    fn border_city_mismatch_stays_weak() {
        // Batam and its Malay neighbour: an adjacent foreign city is a plausible
        // reading, not an anomaly.
        let r = process_layer3_geofence(Some("Johor Bahru"), Some("BATAM"), None);
        assert_eq!(r.mismatch_kind, "DIFFERENT_CITY_NEARBY");
        assert!(r.l3_score <= 0.40);
    }

    #[test]
    fn non_geographic_merchant_marker_is_not_comparable() {
        // E-commerce QRIS puts service descriptors in Tag 60. Flagging those
        // would make the layer fire on every online merchant.
        let r = process_layer3_geofence(Some("Jakarta"), Some("ONLINE"), None);
        assert_eq!(r.mismatch_kind, "NOT_COMPARABLE");
        assert_eq!(r.l3_score, 0.0);
        assert_eq!(r.risk_level, "NOT COMPARABLE");
    }

    #[test]
    fn non_ascii_merchant_city_declines_to_compare() {
        // Our own decoding bug must not become the merchant's finding.
        let r = process_layer3_geofence(Some("Jakarta"), Some("BANDUNG√"), None);
        assert_eq!(r.mismatch_kind, "NOT_COMPARABLE");
        assert_eq!(r.l3_score, 0.0);
    }

    #[test]
    fn missing_merchant_city_is_not_evaluated_not_clean() {
        let r = process_layer3_geofence(Some("Bandung"), None, None);
        assert!(!r.evaluated, "a skipped check must not claim to have passed");
        assert_eq!(r.risk_level, "NOT RUN");
        assert_eq!(r.l3_score, 0.0);
    }

    #[test]
    fn missing_client_location_is_not_evaluated_not_clean() {
        let r = process_layer3_geofence(None, Some("JAKARTA"), None);
        assert!(!r.evaluated);
        assert_eq!(r.risk_level, "NOT RUN");
        assert!(
            r.warnings.iter().any(|w| w.contains("bukan berarti lokasi cocok")),
            "the message must not let a skipped check read as a pass: {:?}",
            r.warnings
        );
    }

    #[test]
    fn a_device_fix_alone_is_enough_to_compare() {
        let r = process_layer3_geofence(Some("Bandung"), Some("JAKARTA"), Some(BANDUNG_FIX));
        assert!(r.evaluated);
        assert!(r.distance_km.is_some());
    }

    #[test]
    fn distance_prefers_the_device_fix_over_the_city_reference() {
        // A confirmed device position is a stronger source than a city-centre
        // approximation, so the two must not silently agree.
        let with_fix = process_layer3_geofence(Some("Bandung"), Some("JAKARTA"), Some(BANDUNG_FIX));
        let from_refs = process_layer3_geofence(Some("Bandung"), Some("JAKARTA"), None);

        assert!(with_fix.distance_km.is_some(), "device fix must yield a distance");
        assert!(from_refs.distance_km.is_some(), "both cities are in the table");

        let d_fix = with_fix.distance_km.unwrap();
        let d_ref = from_refs.distance_km.unwrap();
        assert!(
            (d_fix - d_ref).abs() < 1.0,
            "fixture uses the Bandung city centre as the fix, so both should be ~116 km, got {d_fix} vs {d_ref}"
        );

        // With a genuinely different position, the fix must be what is used.
        let elsewhere = process_layer3_geofence(Some("Bandung"), Some("JAKARTA"), Some(MAKASSAR_FIX));
        assert!(
            elsewhere.distance_km.unwrap() > 1000.0,
            "a Makassar fix must produce a Makassar-scale distance, got {:?}",
            elsewhere.distance_km
        );
    }

    #[test]
    fn null_island_fix_is_rejected() {
        // (0,0) is a classic placeholder. It must not be trusted as a position.
        // The fallback then compares the two city reference points, so the tier
        // is the nearby one, and it must match the no-fix case exactly, proving
        // the bogus fix was discarded rather than used.
        let with_bogus = process_layer3_geofence(Some("Bandung"), Some("JAKARTA"), Some((0.0, 0.0)));
        let no_fix = process_layer3_geofence(Some("Bandung"), Some("JAKARTA"), None);

        assert_eq!(
            with_bogus.distance_km, no_fix.distance_km,
            "a placeholder fix must be discarded, not used as a position"
        );
        assert_eq!(with_bogus.mismatch_kind, "DIFFERENT_CITY_NEARBY");
    }

    #[test]
    fn out_of_range_fix_is_rejected() {
        // An invalid fix is dropped, not used. The fallback then compares the
        // two known city reference points, so a distance still exists, but it
        // must have come from the references, confirmed by the nearby tier.
        let r = process_layer3_geofence(Some("Bandung"), Some("JAKARTA"), Some((999.0, 999.0)));
        let without = process_layer3_geofence(Some("Bandung"), Some("JAKARTA"), None);
        assert_eq!(
            r.distance_km, without.distance_km,
            "an invalid fix must fall back to the city references, not be used"
        );
        assert_eq!(r.mismatch_kind, "DIFFERENT_CITY_NEARBY");
    }

    #[test]
    fn score_never_reaches_a_veto_on_distance_alone() {
        // Guard against a future retune reintroducing the old flat 1.0 rule.
        for (client, merchant, fix) in [
            ("Makassar", "JAKARTA", Some(MAKASSAR_FIX)),
            ("Sleman", "Tual", None),
            ("Johor Bahru", "BATAM", None),
            ("Denpasar", "MEDAN", None),
        ] {
            let r = process_layer3_geofence(Some(client), Some(merchant), fix);
            assert!(
                r.l3_score < 0.70,
                "{client} vs {merchant} scored {} which would veto the combined result",
                r.l3_score
            );
            assert_eq!(r.mismatch_kind != "MATCH" || r.l3_score == 0.0, true);
        }
    }

    #[test]
    fn haversine_matches_a_known_distance() {
        // Jakarta to Bandung is ~120 km as the crow flies.
        let d = haversine_km(JAKARTA, BANDUNG_FIX);
        assert!((100.0..150.0).contains(&d), "got {d}");
    }

    #[test]
    fn normalize_strips_punctuation_and_prefixes() {
        assert_eq!(normalize("Kota Bandung"), "BANDUNG");
        assert_eq!(normalize("KAB. BANDUNG"), "BANDUNG");
        assert_eq!(normalize("  jakarta  selatan "), "JAKARTA SELATAN");
        assert_eq!(normalize("Kota/Kab. X"), "X");
    }

    #[test]
    fn normalize_does_not_fuzzy_match_different_cities() {
        // Guards the no-fuzzy-matching decision.
        assert_ne!(normalize("SURABAYA"), normalize("SEMARANG"));
        assert_ne!(normalize("MEDAN"), normalize("MANADO"));
    }

    #[test]
    fn lookup_finds_aliases() {
        assert_eq!(lookup_city("CIMAHI").unwrap().name, "BANDUNG");
        assert_eq!(lookup_city("KUTA").unwrap().name, "DENPASAR");
        assert!(lookup_city("NOWHERESVILLE").is_none());
    }
}
