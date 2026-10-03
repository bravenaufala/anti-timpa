//! Offline city coordinates, for turning a typed city name into a position.
//!
//! # Relationship to `layer3_geofence::CITIES`
//!
//! The table in `layer3_geofence` holds *reference points used for distance
//! measurement*: a small, hand-checked set, plus the alias and border metadata the
//! scoring tiers need. This table serves a different question ("the user typed
//! 'bandung'; what are its coordinates?") and is allowed to be broader and
//! looser, because a wrong coordinate here produces a wrong distance, whereas a
//! wrong entry in the scoring table produces a wrong verdict.
//!
//! # A seed table instead of a full gazetteer
//!
//! The full Indonesian administrative dataset is ~514 kabupaten/kota. Shipping it
//! would mean a download or a large embedded blob, both of which are out of scope
//! for a build that has no server. A seed table covering the major cities, plus a
//! comment on where to extend it, is an interim measure: it makes the desktop
//! path work for a demo, and it does not pretend to be complete.
//!
//! Lookups are exact-after-normalisation rather than fuzzy. A fuzzy matcher that
//! resolves an unknown city to a *nearby* one would invent a distance, and
//! inventing a distance changes a risk verdict.

/// A city and its approximate centre.
struct CityCoord {
    /// Canonical uppercase name, matching the form used in EMVCo Tag 60.
    name: &'static str,
    /// Additional spellings that resolve to this city.
    aliases: &'static [&'static str],
    lat: f64,
    lon: f64,
}

/// Seed table. Extend this rather than adding fuzzy matching.
///
/// Coordinates are city-centre approximations at ~3 decimal places, which is
/// ~100 m and far finer than a distance tier boundary needs.
const SEED: &[CityCoord] = &[
    CityCoord { name: "JAKARTA", aliases: &["JAKARTA PUSAT", "JAKARTA SELATAN", "JAKARTA BARAT", "JAKARTA TIMUR", "JAKARTA UTARA", "DKI JAKARTA"], lat: -6.2088, lon: 106.8456 },
    CityCoord { name: "BOGOR", aliases: &[], lat: -6.5971, lon: 106.8060 },
    CityCoord { name: "DEPOK", aliases: &[], lat: -6.4025, lon: 106.7942 },
    CityCoord { name: "TANGERANG", aliases: &["TANGERANG SELATAN", "TANGSEL"], lat: -6.1783, lon: 106.6320 },
    CityCoord { name: "BEKASI", aliases: &[], lat: -6.2383, lon: 106.9756 },
    CityCoord { name: "BANDUNG", aliases: &["CIMAHI"], lat: -6.9175, lon: 107.6191 },
    CityCoord { name: "SUKABUMI", aliases: &[], lat: -6.9277, lon: 106.9299 },
    CityCoord { name: "TASIKMALAYA", aliases: &[], lat: -7.3274, lon: 108.2207 },
    CityCoord { name: "CIREBON", aliases: &[], lat: -6.7320, lon: 108.5523 },
    CityCoord { name: "SEMARANG", aliases: &["UNGARAN"], lat: -6.9667, lon: 110.4167 },
    CityCoord { name: "SOLO", aliases: &["SURAKARTA"], lat: -7.5755, lon: 110.8243 },
    CityCoord { name: "YOGYAKARTA", aliases: &["JOGJAKARTA", "JOGJA", "SLEMAN", "BANTUL"], lat: -7.7956, lon: 110.3695 },
    CityCoord { name: "MAGELANG", aliases: &[], lat: -7.4708, lon: 110.2177 },
    CityCoord { name: "PURWOKERTO", aliases: &[], lat: -7.4249, lon: 109.2396 },
    CityCoord { name: "SURABAYA", aliases: &["SIDOARJO"], lat: -7.2575, lon: 112.7521 },
    CityCoord { name: "MALANG", aliases: &[], lat: -7.9666, lon: 112.6326 },
    CityCoord { name: "KEDIRI", aliases: &[], lat: -7.8480, lon: 112.0178 },
    CityCoord { name: "JEMBER", aliases: &[], lat: -8.1845, lon: 113.6681 },
    CityCoord { name: "DENPASAR", aliases: &["BADUNG", "KUTA", "BALI"], lat: -8.6705, lon: 115.2126 },
    CityCoord { name: "MATARAM", aliases: &[], lat: -8.5833, lon: 116.1167 },
    CityCoord { name: "KUPANG", aliases: &[], lat: -10.1772, lon: 123.6070 },
    CityCoord { name: "MEDAN", aliases: &[], lat: 3.5952, lon: 98.6722 },
    CityCoord { name: "PADANG", aliases: &[], lat: -0.9471, lon: 100.4172 },
    CityCoord { name: "PALEMBANG", aliases: &[], lat: -2.9761, lon: 104.7754 },
    CityCoord { name: "PEKANBARU", aliases: &[], lat: 0.5071, lon: 101.4478 },
    CityCoord { name: "BANDAR LAMPUNG", aliases: &["LAMPUNG"], lat: -5.3971, lon: 105.2668 },
    CityCoord { name: "BATAM", aliases: &[], lat: 1.0456, lon: 104.0305 },
    CityCoord { name: "PONTIANAK", aliases: &[], lat: -0.0263, lon: 109.3425 },
    CityCoord { name: "BANJARMASIN", aliases: &[], lat: -3.3186, lon: 114.5944 },
    CityCoord { name: "PALANGKARAYA", aliases: &[], lat: -2.2080, lon: 113.9165 },
    CityCoord { name: "BALIKPAPAN", aliases: &[], lat: -1.2379, lon: 116.8529 },
    CityCoord { name: "SAMARINDA", aliases: &[], lat: -0.5022, lon: 117.1536 },
    CityCoord { name: "MAKASSAR", aliases: &["UJUNG PANDANG"], lat: -5.1477, lon: 119.4327 },
    CityCoord { name: "PALU", aliases: &[], lat: -0.8917, lon: 119.8707 },
    CityCoord { name: "MANADO", aliases: &[], lat: 1.4748, lon: 124.8421 },
    CityCoord { name: "KENDARI", aliases: &[], lat: -3.9450, lon: 122.4989 },
    CityCoord { name: "AMBON", aliases: &[], lat: -3.6954, lon: 128.1814 },
    CityCoord { name: "JAYAPURA", aliases: &[], lat: -2.5916, lon: 140.6690 },
    CityCoord { name: "SORONG", aliases: &[], lat: -0.8762, lon: 131.2558 },
];

/// Administrative prefixes and filler words with no geographic meaning.
const STRIP: [&str; 8] = [
    "KOTA", "KABUPATEN", "KAB", "KOTAMADYA", "MUNICIPALITY", "CITY", "REGENCY", "DISTRICT",
];

/// Normalises a typed city name into a lookup token.
///
/// Mirrors `layer3_geofence::normalize` so that a name which resolves there also
/// resolves here. Kept as a separate small implementation rather than shared,
/// because the two tables are allowed to diverge and coupling them would make a
/// change to one silently affect the other.
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
        if STRIP.contains(&word) {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Resolves a typed city name to `(lat, lon)`.
///
/// Returns `None` for an unknown city. That is an intended outcome, not a
/// failure: Layer 3 then reports `DIFFERENT_CITY_UNBOUNDED` and says the distance
/// could not be computed, rather than substituting a guess.
pub fn coords_for(city: &str) -> Option<(f64, f64)> {
    let key = normalize(city);
    if key.is_empty() {
        return None;
    }
    SEED.iter()
        .find(|c| c.name == key || c.aliases.contains(&key.as_str()))
        .map(|c| (c.lat, c.lon))
}

/// Every canonical name in the table, for UI hints and diagnostics.
pub fn known_cities() -> Vec<&'static str> {
    SEED.iter().map(|c| c.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_a_canonical_name() {
        let (lat, lon) = coords_for("BANDUNG").expect("Bandung is in the table");
        assert!((lat - -6.9175).abs() < 0.01);
        assert!((lon - 107.6191).abs() < 0.01);
    }

    #[test]
    fn resolves_with_administrative_prefixes() {
        // The form a user is most likely to type.
        assert!(coords_for("Kota Bandung").is_some());
        assert!(coords_for("KABUPATEN BANDUNG").is_some());
        assert_eq!(coords_for("Kota Bandung"), coords_for("BANDUNG"));
    }

    #[test]
    fn resolves_case_and_spacing_insensitively() {
        assert_eq!(coords_for("  jakarta  "), coords_for("JAKARTA"));
        assert_eq!(coords_for("bandung"), coords_for("BANDUNG"));
    }

    #[test]
    fn resolves_aliases() {
        assert_eq!(coords_for("JOGJA"), coords_for("YOGYAKARTA"));
        assert_eq!(coords_for("SURAKARTA"), coords_for("SOLO"));
        assert_eq!(coords_for("TANGSEL"), coords_for("TANGERANG"));
        assert_eq!(coords_for("UJUNG PANDANG"), coords_for("MAKASSAR"));
    }

    #[test]
    fn resolves_sub_city_districts_to_their_metro() {
        assert_eq!(coords_for("JAKARTA SELATAN"), coords_for("JAKARTA"));
        assert_eq!(coords_for("KUTA"), coords_for("DENPASAR"));
    }

    #[test]
    fn unknown_city_returns_none_rather_than_a_guess() {
        // The important negative case: an unresolvable city must not be silently
        // mapped to something nearby, because that would fabricate a distance and
        // therefore change a risk verdict.
        assert!(coords_for("NOWHERESVILLE").is_none());
        assert!(coords_for("").is_none());
        assert!(coords_for("   ").is_none());
    }

    #[test]
    fn does_not_fuzzy_match_similar_names() {
        // Guards the no-fuzzy-matching decision at the table level too.
        assert_ne!(coords_for("MALANG"), coords_for("MAGELANG"));
        assert_ne!(coords_for("MEDAN"), coords_for("MANADO"));
        assert_ne!(coords_for("SOLO"), coords_for("SORONG"));
    }

    #[test]
    fn every_entry_has_plausible_indonesian_coordinates() {
        // Catches a typo that would put a city in the ocean or another country.
        for c in SEED {
            assert!(
                (-11.5..=6.5).contains(&c.lat),
                "{} latitude {} is outside Indonesia",
                c.name,
                c.lat
            );
            assert!(
                (94.0..=141.5).contains(&c.lon),
                "{} longitude {} is outside Indonesia",
                c.name,
                c.lon
            );
        }
    }

    #[test]
    fn names_and_aliases_are_unique() {
        // A duplicate would make lookup order decide the result.
        let mut seen: Vec<String> = Vec::new();
        for c in SEED {
            for key in std::iter::once(c.name).chain(c.aliases.iter().copied()) {
                let k = normalize(key);
                assert!(
                    !seen.contains(&k),
                    "'{k}' appears more than once; lookup would be order-dependent"
                );
                seen.push(k);
            }
        }
    }

    #[test]
    fn known_cities_is_not_empty() {
        assert!(known_cities().len() >= 30);
    }
}
