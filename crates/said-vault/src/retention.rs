//! Vault retention + legal-hold — per-document governance state.
//!
//! Each document may carry a `RetentionRecord` stored as a frame
//! `vault:retention:<doc_id>`. It records a retention class, an optional
//! disposition date (after which the doc is eligible for deletion), and a
//! legal-hold flag. A document under legal hold is UNDELETABLE — `deletable`
//! returns a refusal, and the delete path (Feature 3) must honor it. This is
//! the table-stakes governance surface for regulated buyers.
//!
//! Stored as its own frame (not folded into the Manifest) so governance state
//! is independently set/cleared and audited without rewriting the manifest.

use serde::{Deserialize, Serialize};

use sca_core::frames::Pillar;
use sca_core::said_file::SaidFile;

/// Per-document retention + hold state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetentionRecord {
    pub doc_id: String,
    /// Retention class label: "none" | "standard" | "long" | "permanent" |
    /// custom. Free-form so policy schemes can extend without code changes.
    pub class: String,
    /// Unix seconds after which the document is eligible for disposition.
    /// 0 = no disposition date set. Ignored while `legal_hold` is true.
    pub disposition_at: u64,
    /// When true, the document CANNOT be deleted or compacted, regardless of
    /// disposition date. Set by legal/compliance; overrides everything.
    pub legal_hold: bool,
    /// Free-form reason (matter number, regulation, ticket).
    pub reason: String,
    /// Unix seconds this record was last updated.
    pub updated_at: u64,
}

impl RetentionRecord {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("RetentionRecord serde must not fail")
    }
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

fn retention_frame_id(doc_id: &str) -> String {
    format!("vault:retention:{}", doc_id)
}

/// Read a document's retention record, if any.
pub fn get(brain: &mut SaidFile, doc_id: &str) -> Option<RetentionRecord> {
    brain
        .read(&retention_frame_id(doc_id))
        .and_then(|j| RetentionRecord::from_json(&j).ok())
}

/// Set (create or replace) a document's retention class + disposition date.
/// Leaves the existing legal-hold flag unchanged (use `set_hold` for that).
pub fn set_retention(
    brain: &mut SaidFile,
    doc_id: &str,
    class: &str,
    disposition_at: u64,
    reason: &str,
) -> RetentionRecord {
    let legal_hold = get(brain, doc_id).map(|r| r.legal_hold).unwrap_or(false);
    let rec = RetentionRecord {
        doc_id: doc_id.to_string(),
        class: class.to_string(),
        disposition_at,
        legal_hold,
        reason: reason.to_string(),
        updated_at: sca_core::time_compat::unix_secs(),
    };
    write(brain, &rec);
    rec
}

/// Place or release a legal hold, preserving the retention class/date.
pub fn set_hold(brain: &mut SaidFile, doc_id: &str, hold: bool, reason: &str) -> RetentionRecord {
    let existing = get(brain, doc_id);
    let rec = RetentionRecord {
        doc_id: doc_id.to_string(),
        class: existing.as_ref().map(|r| r.class.clone()).unwrap_or_else(|| "none".into()),
        disposition_at: existing.as_ref().map(|r| r.disposition_at).unwrap_or(0),
        legal_hold: hold,
        reason: reason.to_string(),
        updated_at: sca_core::time_compat::unix_secs(),
    };
    write(brain, &rec);
    rec
}

fn write(brain: &mut SaidFile, rec: &RetentionRecord) {
    let tags = vec![
        "vault:retention".to_string(),
        format!("vault:retention:class:{}", rec.class),
    ];
    brain.remember_with_pillar(
        Some(&retention_frame_id(&rec.doc_id)),
        &rec.to_json(),
        None,
        Pillar::Document,
        tags,
    );
}

/// Whether a document may be deleted right now, with a human-readable reason
/// when it may not. Deletion is refused when:
///   - a legal hold is active (always wins), or
///   - a disposition date is set and has not yet passed.
/// A document with no retention record is freely deletable.
pub struct Deletable {
    pub ok: bool,
    pub reason: String,
}

pub fn deletable(brain: &mut SaidFile, doc_id: &str) -> Deletable {
    match get(brain, doc_id) {
        None => Deletable { ok: true, reason: String::new() },
        Some(r) => {
            if r.legal_hold {
                return Deletable {
                    ok: false,
                    reason: if r.reason.is_empty() {
                        "under legal hold".into()
                    } else {
                        format!("under legal hold: {}", r.reason)
                    },
                };
            }
            if r.disposition_at != 0 {
                let now = sca_core::time_compat::unix_secs();
                if now < r.disposition_at {
                    return Deletable {
                        ok: false,
                        reason: format!(
                            "retained until disposition date (class: {})",
                            r.class
                        ),
                    };
                }
            }
            Deletable { ok: true, reason: String::new() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_brain(tag: &str) -> SaidFile {
        let p = std::env::temp_dir()
            .join(format!("vault_ret_{}.said", tag))
            .to_string_lossy()
            .into_owned();
        let _ = std::fs::remove_file(&p);
        SaidFile::create(&p)
    }

    #[test]
    fn no_record_is_deletable() {
        let mut b = tmp_brain("none");
        assert!(deletable(&mut b, "doc1").ok);
    }

    #[test]
    fn legal_hold_blocks_deletion() {
        let mut b = tmp_brain("hold");
        set_hold(&mut b, "doc1", true, "matter 1234");
        let d = deletable(&mut b, "doc1");
        assert!(!d.ok);
        assert!(d.reason.contains("legal hold"));
        // Releasing the hold makes it deletable again.
        set_hold(&mut b, "doc1", false, "released");
        assert!(deletable(&mut b, "doc1").ok);
    }

    #[test]
    fn future_disposition_blocks_deletion() {
        let mut b = tmp_brain("dispo");
        let future = sca_core::time_compat::unix_secs() + 86_400;
        set_retention(&mut b, "doc1", "standard", future, "policy");
        assert!(!deletable(&mut b, "doc1").ok);
    }

    #[test]
    fn hold_survives_retention_change() {
        let mut b = tmp_brain("survive");
        set_hold(&mut b, "doc1", true, "hold");
        // Setting retention class must not clear the hold.
        set_retention(&mut b, "doc1", "standard", 0, "policy");
        assert!(get(&mut b, "doc1").unwrap().legal_hold);
        assert!(!deletable(&mut b, "doc1").ok);
    }
}
