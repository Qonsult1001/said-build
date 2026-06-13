//! Vault audit log — an immutable, append-only, hash-chained event log.
//!
//! Every consequential vault action (ingest, view, restore, verify, export,
//! delete) is recorded as one frame `vault:audit:<seq>` whose JSON includes
//! the BLAKE3 of the previous event. Tampering with or removing any event
//! breaks the chain, which `verify_chain` detects — this is the
//! chain-of-custody surface an enterprise/compliance auditor expects.
//!
//! Storage mirrors the manifest pattern: plain frames the WASM brain already
//! knows how to read/write, so no new section format is needed.

use serde::{Deserialize, Serialize};

use crate::hasher::blake3_hex;
use sca_core::frames::Pillar;
use sca_core::said_file::SaidFile;

/// One audit event. `seq` is monotonic from 0. `prev_hash` is the BLAKE3 of
/// the previous event's canonical JSON ("" for the genesis event). `hash` is
/// NOT stored in the frame — it's recomputed on read so the chain can't be
/// forged by editing a stored hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditEvent {
    pub seq: u64,
    /// Action verb: "ingest" | "view" | "restore" | "verify" | "export" | "delete".
    pub action: String,
    /// Target document doc_id (BLAKE3 of original), or "" for vault-wide events.
    pub doc_id: String,
    /// Human-readable target (filename) when known.
    pub target: String,
    /// Authenticated actor identity.
    pub actor: String,
    /// Unix seconds (wall clock via time_compat — works on wasm + native).
    pub at: u64,
    /// BLAKE3 of the previous event's canonical JSON ("" for genesis).
    pub prev_hash: String,
    /// Free-form detail (e.g. "legal", "verified ok", "blocked: legal hold").
    pub note: String,
}

impl AuditEvent {
    /// Canonical JSON used both for storage and for chain hashing. Field order
    /// is fixed by the struct definition, so the bytes are deterministic.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("AuditEvent serde must not fail")
    }
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
    /// The chain hash of this event = BLAKE3 of its canonical JSON.
    pub fn chain_hash(&self) -> String {
        blake3_hex(self.to_json().as_bytes())
    }
}

fn audit_frame_id(seq: u64) -> String {
    // Zero-pad so lexical doc_id order == numeric order for the 12-digit range.
    format!("vault:audit:{:012}", seq)
}

/// Read every audit event in sequence order.
pub fn list(brain: &mut SaidFile) -> Vec<AuditEvent> {
    let ids: Vec<String> = brain
        .frames
        .active_doc_ids()
        .iter()
        .filter(|d| d.starts_with("vault:audit:"))
        .map(|s| s.to_string())
        .collect();
    let mut out: Vec<AuditEvent> = Vec::new();
    for id in ids {
        if let Some(json) = brain.read(&id) {
            if let Ok(ev) = AuditEvent::from_json(&json) {
                out.push(ev);
            }
        }
    }
    out.sort_by_key(|e| e.seq);
    out
}

/// The next sequence number = current event count.
fn next_seq(brain: &mut SaidFile) -> u64 {
    list(brain).len() as u64
}

/// Append an event to the chain. Computes `seq` and `prev_hash` from the
/// existing log, stamps `at` from the wall clock, and writes one frame.
/// Returns the appended event.
pub fn append(
    brain: &mut SaidFile,
    action: &str,
    doc_id: &str,
    target: &str,
    actor: &str,
    note: &str,
) -> AuditEvent {
    let existing = list(brain);
    let seq = existing.len() as u64;
    let prev_hash = existing.last().map(|e| e.chain_hash()).unwrap_or_default();
    let ev = AuditEvent {
        seq,
        action: action.to_string(),
        doc_id: doc_id.to_string(),
        target: target.to_string(),
        actor: actor.to_string(),
        at: sca_core::time_compat::unix_secs(),
        prev_hash,
        note: note.to_string(),
    };
    let frame_id = audit_frame_id(seq);
    let tags = vec!["vault:audit".to_string(), format!("vault:audit:action:{}", action)];
    brain.remember_with_pillar(Some(&frame_id), &ev.to_json(), None, Pillar::Document, tags);
    ev
}

/// Result of a chain-integrity check.
#[derive(Debug, Clone, Serialize)]
pub struct ChainStatus {
    pub ok: bool,
    pub count: usize,
    /// 1-based index of the first broken link, or 0 if the chain is intact.
    pub broken_at: u64,
    pub detail: String,
}

/// Verify the hash chain: every event's `prev_hash` must equal the previous
/// event's recomputed `chain_hash`, and `seq` must be contiguous from 0. Any
/// deviation (edited event, removed event, reordered event) breaks it.
pub fn verify_chain(brain: &mut SaidFile) -> ChainStatus {
    let events = list(brain);
    let mut prev_hash = String::new();
    for (i, ev) in events.iter().enumerate() {
        if ev.seq != i as u64 {
            return ChainStatus {
                ok: false,
                count: events.len(),
                broken_at: (i as u64) + 1,
                detail: format!("seq gap at index {} (got seq {})", i, ev.seq),
            };
        }
        if ev.prev_hash != prev_hash {
            return ChainStatus {
                ok: false,
                count: events.len(),
                broken_at: (i as u64) + 1,
                detail: format!("prev_hash mismatch at event {}", ev.seq),
            };
        }
        prev_hash = ev.chain_hash();
    }
    ChainStatus {
        ok: true,
        count: events.len(),
        broken_at: 0,
        detail: if events.is_empty() {
            "no events yet".into()
        } else {
            format!("{} events, chain intact", events.len())
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_brain(tag: &str) -> SaidFile {
        let p = std::env::temp_dir()
            .join(format!("vault_audit_{}.said", tag))
            .to_string_lossy()
            .into_owned();
        let _ = std::fs::remove_file(&p);
        SaidFile::create(&p)
    }

    #[test]
    fn appends_and_chains() {
        let mut b = tmp_brain("chain");
        append(&mut b, "ingest", "doc1", "a.docx", "alice", "legal");
        append(&mut b, "restore", "doc1", "a.docx", "alice", "ok");
        let events = list(&mut b);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[0].prev_hash, "");
        // Second event's prev_hash must equal the first's chain hash.
        assert_eq!(events[1].prev_hash, events[0].chain_hash());
        assert!(verify_chain(&mut b).ok);
    }

    #[test]
    fn empty_chain_is_ok() {
        let mut b = tmp_brain("empty");
        let s = verify_chain(&mut b);
        assert!(s.ok);
        assert_eq!(s.count, 0);
    }
}
