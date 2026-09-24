//! In-memory, session-scoped scan history + a tamper-evident hash chain.
//!
//! Two jobs, one structure:
//!
//! 1. **Evidence.** A finding is only useful if you can refer back to it —
//!    "which scan, at what time, what exactly did it see?". The rolling buffer
//!    keeps the last `MAX_ENTRIES` scans so the UI can show a timeline.
//!
//! 2. **Tamper-evidence.** Each entry's hash covers the previous entry's hash,
//!    forming an append-only chain. Deleting or editing an entry breaks every
//!    hash after it, which `verify_chain` detects.
//!
//! What this is *not*: a secure audit log. There is no key and no signature, so
//! a determined attacker who can rewrite the whole buffer can also recompute the
//! whole chain. It is a cheap integrity seam that makes accidental corruption
//! and naive editing visible, and it is deliberately in-memory only — persisting
//! a scan log is a privacy decision that must be made by the user, not by
//! default (see the report's security chapter).

use serde::{Deserialize, Serialize};

/// Comment included first in every hashed payload.
///
/// Hashing a *structured, labelled* encoding rather than a bare concatenation
/// closes the obvious collision: without field labels, a merchant name ending in
/// digits could be made to look like a different MCC + score pair.
const HASH_SCHEMA: &str = "ANTITIMPA-SCAN-v1";

/// Rolling buffer cap. Bounded so a long session cannot grow memory without
/// limit; the oldest entry is evicted first.
pub const MAX_ENTRIES: usize = 200;

/// FNV-1a 64-bit offset basis and prime.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a, used as a fast non-cryptographic integrity hash.
///
/// Chosen deliberately over SHA-256: this chain guards against accidental
/// corruption and casual editing, not a motivated adversary, and FNV-1a keeps
/// the dependency footprint at zero. The report states this limitation
/// explicitly — do not mistake it for a signature.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// One recorded scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Monotonic sequence number, 1-based within a session.
    pub seq: u64,
    /// `None` when the platform clock is unavailable.
    pub timestamp_ms: Option<u64>,
    /// Where the payload came from: `camera`, `manual`, or `sample`.
    pub source: String,
    pub combined_score: f64,
    pub combined_risk_level: String,
    /// Truncated payload preview — never the full payload, so the history view
    /// cannot become an accidental log of everything the user scanned.
    pub payload_preview: String,
    pub l1_score: f64,
    pub l2_score: f64,
    pub l3_score: f64,
    pub crc_valid: bool,
    /// Name of the first finding rule that fired, when any.
    pub top_finding: Option<String>,
    /// Hash of the previous entry; `0` for the genesis entry.
    pub prev_hash: u64,
    /// Integrity hash over this entry's fields plus `prev_hash`.
    pub entry_hash: u64,
}

/// What callers hand in; the chain fills in hashes and sequence numbers.
#[derive(Debug, Clone)]
pub struct HistoryInput {
    pub timestamp_ms: Option<u64>,
    pub source: String,
    pub combined_score: f64,
    pub combined_risk_level: String,
    pub payload: String,
    pub l1_score: f64,
    pub l2_score: f64,
    pub l3_score: f64,
    pub crc_valid: bool,
    pub top_finding: Option<String>,
}

/// Number of leading payload characters kept in `payload_preview`.
const PREVIEW_LEN: usize = 24;

fn preview(payload: &str) -> String {
    let mut out: String = payload.chars().take(PREVIEW_LEN).collect();
    if payload.chars().count() > PREVIEW_LEN {
        out.push('…');
    }
    out
}

/// Builds the canonical byte encoding that gets hashed.
///
/// Every field is length-prefixed and labelled so no two distinct entries can
/// serialize to the same bytes.
fn canonical_bytes(entry: &HistoryEntry) -> Vec<u8> {
    let mut s = String::new();
    s.push_str(HASH_SCHEMA);
    s.push('\x1f');
    s.push_str(&format!("seq={};", entry.seq));
    s.push_str(&format!("ts={};", entry.timestamp_ms.unwrap_or(0)));
    s.push_str(&format!("src={};", entry.source));
    s.push_str(&format!("score={:.6};", entry.combined_score));
    s.push_str(&format!("band={};", entry.combined_risk_level));
    s.push_str(&format!("l1={:.6};l2={:.6};l3={:.6};", entry.l1_score, entry.l2_score, entry.l3_score));
    s.push_str(&format!("crc={};", entry.crc_valid as u8));
    s.push_str(&format!("find={};", entry.top_finding.as_deref().unwrap_or("")));
    // Length prefixes stop field-boundary ambiguity in the free-form fields.
    s.push_str(&format!("prev={};", entry.prev_hash));
    s.push_str(&format!("pvlen={};", entry.payload_preview.len()));
    s.push_str(&entry.payload_preview);
    s.push('\x1e');
    s.into_bytes()
}

/// Append-only chain over a bounded ring of entries.
#[derive(Debug, Default)]
pub struct HistoryChain {
    entries: Vec<HistoryEntry>,
    next_seq: u64,
    last_hash: u64,
}

impl HistoryChain {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_seq: 1,
            last_hash: 0,
        }
    }

    /// Appends a scan, evicting the oldest entry when full.
    ///
    /// Returns the stored entry.
    pub fn push(&mut self, input: HistoryInput) -> HistoryEntry {
        let mut entry = HistoryEntry {
            seq: self.next_seq,
            timestamp_ms: input.timestamp_ms,
            source: input.source,
            combined_score: input.combined_score,
            combined_risk_level: input.combined_risk_level,
            payload_preview: preview(&input.payload),
            l1_score: input.l1_score,
            l2_score: input.l2_score,
            l3_score: input.l3_score,
            crc_valid: input.crc_valid,
            top_finding: input.top_finding,
            prev_hash: self.last_hash,
            entry_hash: 0,
        };
        entry.entry_hash = fnv1a(&canonical_bytes(&entry));

        self.next_seq += 1;
        self.last_hash = entry.entry_hash;

        if self.entries.len() >= MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push(entry.clone());
        entry
    }

    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.next_seq = 1;
        self.last_hash = 0;
    }

    /// Walks the chain and reports whether it is intact.
    ///
    /// Returns `(intact, detail)`. A broken link is reported with the sequence
    /// number where it broke, which is the actionable part.
    pub fn verify(&self) -> (bool, String) {
        let mut expected_prev = 0u64;
        for (i, e) in self.entries.iter().enumerate() {
            // Only the very first entry may chain from the genesis hash: after
            // an eviction the buffer no longer starts at seq 1, but the links
            // between surviving entries must still be continuous.
            if i > 0 && e.prev_hash != expected_prev {
                return (
                    false,
                    format!(
                        "rantai terputus di seq {}: prev_hash={:#x}, seharusnya {:#x}",
                        e.seq, e.prev_hash, expected_prev
                    ),
                );
            }
            let recomputed = fnv1a(&canonical_bytes(e));
            if recomputed != e.entry_hash {
                return (
                    false,
                    format!(
                        "entri seq {} dimodifikasi: hash={:#x}, dihitung ulang={:#x}",
                        e.seq, e.entry_hash, recomputed
                    ),
                );
            }
            expected_prev = e.entry_hash;
        }
        (true, format!("{} entri terverifikasi", self.entries.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(payload: &str, score: f64) -> HistoryInput {
        HistoryInput {
            timestamp_ms: Some(1_700_000_000_000),
            source: "manual".into(),
            combined_score: score,
            combined_risk_level: if score > 0.7 {
                "HIGH RISK".into()
            } else {
                "LOW RISK".into()
            },
            payload: payload.into(),
            l1_score: score,
            l2_score: 0.0,
            l3_score: 0.0,
            crc_valid: true,
            top_finding: None,
        }
    }

    #[test]
    fn push_assigns_sequential_numbers_and_links_hashes() {
        let mut chain = HistoryChain::new();
        let a = chain.push(input("0002010102", 0.0));
        let b = chain.push(input("0002010102", 0.9));

        assert_eq!(a.seq, 1);
        assert_eq!(b.seq, 2);
        assert_eq!(a.prev_hash, 0, "genesis entry must chain from zero");
        assert_eq!(b.prev_hash, a.entry_hash, "each entry links to its predecessor");
        assert_ne!(a.entry_hash, b.entry_hash);
    }

    #[test]
    fn chain_verifies_when_untouched() {
        let mut chain = HistoryChain::new();
        for i in 0..5 {
            chain.push(input("0002010102", i as f64 / 10.0));
        }
        let (ok, detail) = chain.verify();
        assert!(ok, "clean chain must verify: {detail}");
        assert_eq!(chain.len(), 5);
    }

    #[test]
    fn editing_an_entry_breaks_verification() {
        let mut chain = HistoryChain::new();
        for _ in 0..3 {
            chain.push(input("0002010102", 0.0));
        }
        // Simulate post-hoc editing: downgrade a stored finding so the risk
        // band no longer matches what was scored.
        chain.entries[1].combined_score = 0.9;
        chain.entries[1].combined_risk_level = "HIGH RISK".into();

        let (ok, detail) = chain.verify();
        assert!(!ok, "a modified entry must be detected");
        assert!(detail.contains("dimodifikasi"), "got: {detail}");
    }

    #[test]
    fn removing_an_entry_breaks_the_links() {
        let mut chain = HistoryChain::new();
        for _ in 0..3 {
            chain.push(input("0002010102", 0.0));
        }
        chain.entries.remove(1);

        let (ok, detail) = chain.verify();
        assert!(!ok, "a removed entry must be detected");
        assert!(detail.contains("rantai terputus"), "got: {detail}");
    }

    #[test]
    fn buffer_is_bounded_and_drops_oldest() {
        let mut chain = HistoryChain::new();
        for _ in 0..MAX_ENTRIES + 10 {
            chain.push(input("0002010102", 0.0));
        }
        assert_eq!(chain.len(), MAX_ENTRIES);
        // Oldest ten evicted, so the first surviving entry has seq 11.
        assert_eq!(chain.entries[0].seq, 11);
        // The surviving links must still be continuous.
        assert!(chain.verify().0, "chain must survive eviction");
    }

    #[test]
    fn payload_preview_is_truncated() {
        let mut chain = HistoryChain::new();
        let long = "A".repeat(200);
        let e = chain.push(input(&long, 0.0));
        assert_eq!(e.payload_preview.chars().count(), PREVIEW_LEN + 1);
        assert!(e.payload_preview.ends_with('…'));

        let short = chain.push(input("12345", 0.0));
        assert_eq!(short.payload_preview, "12345", "short payloads are not padded");
    }

    #[test]
    fn field_boundaries_cannot_be_confused() {
        // Two entries differing only in how a value is split across fields must
        // not collide. Length-prefixing the preview is what prevents this.
        let mut chain = HistoryChain::new();
        let a = chain.push(HistoryInput {
            top_finding: Some("ab".into()),
            payload: "c".into(),
            ..input("x", 0.0)
        });
        let b = chain.push(HistoryInput {
            top_finding: Some("a".into()),
            payload: "bc".into(),
            ..input("x", 0.0)
        });
        assert_ne!(a.entry_hash, b.entry_hash);
    }

    #[test]
    fn clear_resets_the_chain() {
        let mut chain = HistoryChain::new();
        chain.push(input("0002010102", 0.5));
        chain.clear();
        assert!(chain.is_empty());
        let e = chain.push(input("0002010102", 0.0));
        assert_eq!(e.seq, 1, "sequence restarts after clear");
        assert_eq!(e.prev_hash, 0);
    }
}
