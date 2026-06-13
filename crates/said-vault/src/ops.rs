//! Borrowing vault operations over a `&mut SaidFile`. Lets a caller that
//! already owns a SaidFile (e.g. the WASM SaidBrain) run vault ingest/
//! rebuild/restore on THAT file, without said-vault opening a second one.
//! The CLI's owning `SaidVault` and this borrowing path share the same
//! underlying logic.

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

use crate::hasher::blake3_hex;
use crate::manifest::{Manifest, ZipEntry};
use crate::parser;
use crate::rebuild::{self, RebuildRef};

/// Store a vault asset on a borrowed SaidFile (binary-safe, deduped by doc_id).
/// Mirrors SaidStore::put_object but on a borrow.
pub fn put_object(brain: &mut SaidFile, hash: &str, kind: &str, data: &[u8]) -> Result<(), String> {
    let prefix = match kind {
        "paragraph" => "para",
        "image" => "img",
        "font" => "font",
        "xml" | "docx_xml" => "xml",
        other => other,
    };
    let doc_id = format!("vault:{}:{}", prefix, hash);
    if brain.frames.get_meta(&doc_id).is_some() {
        return Ok(());
    }
    let tags = vec!["vault:asset".to_string(), format!("vault:kind:{}", kind)];
    brain.put_binary(&doc_id, data, Pillar::Document, tags);
    Ok(())
}

/// Fetch a vault asset by (kind, hash) directly. Mirrors
/// SaidStore::get_object_kinded on a borrow.
pub fn get_object_kinded(brain: &mut SaidFile, kind: &str, hash: &str) -> Option<Vec<u8>> {
    let prefix = match kind {
        "paragraph" => "para",
        "image" => "img",
        "font" => "font",
        "xml" | "docx_xml" => "xml",
        other => other,
    };
    brain.read_binary(&format!("vault:{}:{}", prefix, hash))
}

/// Ingest a DOCX into the vault on a borrowed SaidFile. `legal=true` stores the
/// byte-exact preflate recipe tombstone; `legal=false` is slim. Returns doc_id.
pub fn ingest_docx(
    brain: &mut SaidFile,
    bytes: &[u8],
    filename: &str,
    legal: bool,
    ingested_by: &str,
) -> Result<String, String> {
    let doc_id = blake3_hex(bytes);
    let manifest_doc_id = format!("vault:manifest:{}", doc_id);
    if brain.read(&manifest_doc_id).is_some() {
        return Ok(doc_id); // idempotent
    }

    let parsed = parser::docx::parse(bytes).map_err(|e| e.to_string())?;
    let mut zip_entries: Vec<ZipEntry> = Vec::new();
    for p in &parsed.paragraphs {
        put_object(brain, &p.hash, "paragraph", p.text.as_bytes())?;
        zip_entries.push(ZipEntry { name: "paragraph".into(), asset_hash: p.hash.clone(), kind: "paragraph".into() });
    }
    for img in &parsed.images {
        put_object(brain, &img.hash, "image", &img.data)?;
        zip_entries.push(ZipEntry { name: img.name.clone(), asset_hash: img.hash.clone(), kind: "image".into() });
    }
    for f in &parsed.fonts {
        put_object(brain, &f.hash, "font", &f.data)?;
        zip_entries.push(ZipEntry { name: f.name.clone(), asset_hash: f.hash.clone(), kind: "font".into() });
    }
    for x in &parsed.structural_xml {
        put_object(brain, &x.hash, "xml", &x.data)?;
        zip_entries.push(ZipEntry { name: x.name.clone(), asset_hash: x.hash.clone(), kind: "xml".into() });
    }

    // Legal-tier tombstone: a 1-byte marker (0x01 preflate recipe / 0x00
    // verbatim) + body. Use a recipe ONLY if it decodes back to the exact
    // original bytes (whole-file double-verify); else store verbatim. This is
    // byte-identical to the owning SaidVault::ingest path — never silently lossy.
    let tombstone_hash = if legal {
        let mut payload = Vec::with_capacity(bytes.len() + 1);
        match crate::preflate_recipe::encode(bytes) {
            Some(recipe) if crate::preflate_recipe::decode(&recipe).ok().as_deref() == Some(bytes) => {
                payload.push(0x01);
                payload.extend_from_slice(&recipe);
            }
            _ => {
                payload.push(0x00);
                payload.extend_from_slice(bytes);
            }
        }
        brain.vault_tombstones_mut().put(&doc_id, &payload);
        // Manifest hash is BLAKE3 of the ORIGINAL file, not the payload.
        Some(blake3_hex(bytes))
    } else {
        None
    };

    let manifest = Manifest {
        doc_id: doc_id.clone(),
        format: "docx".into(),
        filename: filename.to_string(),
        size_bytes: bytes.len() as u64,
        ingested_at: String::new(), // WASM has no clock here; left empty (CLI sets RFC3339)
        ingested_by: ingested_by.to_string(),
        tags: Vec::new(),
        zip_entries,
        tombstone_hash,
    };
    let manifest_json = manifest.to_json();
    let manifest_tags = vec![
        "vault:manifest".to_string(),
        "vault:fmt:docx".to_string(),
        format!("vault:filename:{}", filename),
    ];
    brain.remember_with_pillar(Some(&manifest_doc_id), &manifest_json, None, Pillar::Document, manifest_tags);

    // Chain-of-custody: record the ingest in the append-only audit log.
    crate::audit::append(
        brain,
        "ingest",
        &doc_id,
        filename,
        ingested_by,
        if legal { "legal" } else { "slim" },
    );
    Ok(doc_id)
}

fn load_manifest(brain: &mut SaidFile, doc_id: &str) -> Result<Manifest, String> {
    let mid = format!("vault:manifest:{}", doc_id);
    let json = brain.read(&mid).ok_or_else(|| format!("no manifest for {}", doc_id))?;
    Manifest::from_json(&json).map_err(|e| format!("manifest parse: {}", e))
}

/// Rebuild a document from dedup parts (structural). Returns the bytes.
/// Functionally equivalent to the original but NOT necessarily byte-exact
/// (use `restore_bytes` for byte-exact recovery).
pub fn rebuild_bytes(brain: &mut SaidFile, doc_id: &str) -> Result<Vec<u8>, String> {
    let manifest = load_manifest(brain, doc_id)?;
    let mut refs = Vec::new();
    for entry in &manifest.zip_entries {
        let bytes = get_object_kinded(brain, &entry.kind, &entry.asset_hash)
            .ok_or_else(|| format!("missing {} asset {}", entry.kind, entry.asset_hash))?;
        refs.push(RebuildRef { kind: entry.kind.clone(), name: Some(entry.name.clone()), bytes });
    }
    match manifest.format.as_str() {
        "docx" => rebuild::docx::rebuild(&refs),
        "pdf" => rebuild::pdf::rebuild(&refs),
        other => Err(format!("unsupported format for rebuild: {}", other)),
    }
}

/// Restore the byte-exact original from the tombstone (legal only). Reads the
/// marker (0x01 recipe / 0x00 verbatim), decodes, BLAKE3-verifies against the
/// manifest record. Byte-identical to the owning SaidVault::restore_bytes path.
pub fn restore_bytes(brain: &mut SaidFile, doc_id: &str) -> Result<Vec<u8>, String> {
    let manifest = load_manifest(brain, doc_id)?;
    let expected = manifest.tombstone_hash.clone().ok_or_else(||
        format!("no tombstone for {} (slim mode) — use rebuild", doc_id))?;
    let payload = brain.vault_tombstones_mut().get(doc_id)
        .ok_or_else(|| format!("tombstone for {} missing or BLAKE3-mismatched", doc_id))?;
    if payload.is_empty() { return Err(format!("tombstone for {} empty", doc_id)); }
    let bytes = match payload[0] {
        0x01 => crate::preflate_recipe::decode(&payload[1..]).map_err(|e| format!("recipe decode: {}", e))?,
        0x00 => payload[1..].to_vec(),
        other => return Err(format!("unknown marker {}", other)),
    };
    let actual = blake3_hex(&bytes);
    if actual != expected {
        return Err(format!("BLAKE3 mismatch for {}: expected {} got {}", doc_id, expected, actual));
    }
    Ok(bytes)
}

/// A signed record that a document was erased — the GDPR Art. 17 ("right to
/// erasure") proof. The content is sealed by `seal` = BLAKE3 of the canonical
/// fields, so the certificate is tamper-evident.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct DeletionCertificate {
    pub doc_id: String,
    pub filename: String,
    /// BLAKE3 of the original document (from the manifest), preserved as proof
    /// of WHAT was erased without keeping the content itself.
    pub original_blake3: String,
    pub deleted_by: String,
    pub deleted_at: u64,
    pub reason: String,
    /// Counts of what was removed, for the audit trail.
    pub assets_removed: usize,
    pub tombstone_removed: bool,
    /// BLAKE3 seal over the canonical certificate fields (integrity).
    pub seal: String,
}

impl DeletionCertificate {
    fn seal_input(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.doc_id,
            self.filename,
            self.original_blake3,
            self.deleted_by,
            self.deleted_at,
            self.reason,
            self.assets_removed,
            self.tombstone_removed
        )
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("DeletionCertificate serde must not fail")
    }
}

/// Erase a document from the vault, producing a tamper-evident deletion
/// certificate. REFUSES if the document is under legal hold or before its
/// disposition date (caller should also check, but this is the last line).
///
/// Removes: the manifest, the retention record, the byte-exact tombstone, and
/// any dedup asset frames no longer referenced by another document. Records a
/// `delete` audit event — the chain keeps the *record* of erasure even though
/// the content is gone, which is exactly what GDPR + chain-of-custody need.
pub fn delete_document(
    brain: &mut SaidFile,
    doc_id: &str,
    deleted_by: &str,
    reason: &str,
) -> Result<DeletionCertificate, String> {
    // Last-line governance gate.
    let gate = crate::retention::deletable(brain, doc_id);
    if !gate.ok {
        return Err(format!("deletion refused — {}", gate.reason));
    }

    let manifest = load_manifest(brain, doc_id)?;
    let original_blake3 = manifest
        .tombstone_hash
        .clone()
        .unwrap_or_else(|| manifest.doc_id.clone());

    // Which asset hashes does THIS doc reference, and which are shared with
    // other docs (those must survive — content-addressed dedup).
    let mine: std::collections::HashSet<(String, String)> = manifest
        .zip_entries
        .iter()
        .map(|e| (e.kind.clone(), e.asset_hash.clone()))
        .collect();
    let mut referenced_elsewhere: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    let other_manifest_ids: Vec<String> = brain
        .frames
        .active_doc_ids()
        .iter()
        .filter(|d| d.starts_with("vault:manifest:") && **d != format!("vault:manifest:{}", doc_id))
        .map(|s| s.to_string())
        .collect();
    for mid in other_manifest_ids {
        if let Some(j) = brain.read(&mid) {
            if let Ok(m) = Manifest::from_json(&j) {
                for e in &m.zip_entries {
                    referenced_elsewhere.insert((e.kind.clone(), e.asset_hash.clone()));
                }
            }
        }
    }

    // Remove assets unique to this document.
    let mut assets_removed = 0usize;
    for (kind, hash) in &mine {
        if referenced_elsewhere.contains(&(kind.clone(), hash.clone())) {
            continue; // shared — keep
        }
        let prefix = match kind.as_str() {
            "paragraph" => "para",
            "image" => "img",
            "font" => "font",
            "xml" | "docx_xml" => "xml",
            other => other,
        };
        if brain.forget(&format!("vault:{}:{}", prefix, hash)) {
            assets_removed += 1;
        }
    }

    // Remove the byte-exact tombstone (the sensitive payload).
    let tombstone_removed = manifest.tombstone_hash.is_some()
        && brain.vault_tombstones_mut().remove(doc_id).is_ok();

    // Remove the manifest + retention record.
    brain.forget(&format!("vault:manifest:{}", doc_id));
    brain.forget(&format!("vault:retention:{}", doc_id));

    let mut cert = DeletionCertificate {
        doc_id: doc_id.to_string(),
        filename: manifest.filename.clone(),
        original_blake3,
        deleted_by: deleted_by.to_string(),
        deleted_at: sca_core::time_compat::unix_secs(),
        reason: reason.to_string(),
        assets_removed,
        tombstone_removed,
        seal: String::new(),
    };
    cert.seal = blake3_hex(cert.seal_input().as_bytes());

    // Retain the certificate IN-SYSTEM as the erasure register entry. The
    // content is gone but the proof-of-deletion survives in the .said file, so
    // an auditor can later list every erasure without external files. The
    // doc_id no longer collides with anything (manifest/assets removed above).
    let cert_tags = vec!["vault:cert".to_string(), format!("vault:cert:doc:{}", doc_id)];
    brain.remember_with_pillar(
        Some(&format!("vault:cert:{}", doc_id)),
        &cert.to_json(),
        None,
        Pillar::Document,
        cert_tags,
    );

    // Record the erasure in the append-only audit log (content gone, record kept).
    crate::audit::append(
        brain,
        "delete",
        doc_id,
        &manifest.filename,
        deleted_by,
        &format!("erased · {} assets · seal {}", assets_removed, &cert.seal[..16.min(cert.seal.len())]),
    );

    Ok(cert)
}

/// The erasure register: every retained deletion certificate, newest first.
/// Each entry carries a `seal_valid` flag recomputed from its fields, so the
/// register is self-verifying (a tampered certificate shows as invalid).
pub fn list_certificates(brain: &mut SaidFile) -> Vec<DeletionCertificate> {
    let ids: Vec<String> = brain
        .frames
        .active_doc_ids()
        .iter()
        .filter(|d| d.starts_with("vault:cert:") && !d.starts_with("vault:cert:doc:"))
        .map(|s| s.to_string())
        .collect();
    let mut out: Vec<DeletionCertificate> = Vec::new();
    for id in ids {
        if let Some(json) = brain.read(&id) {
            if let Ok(c) = serde_json::from_str::<DeletionCertificate>(&json) {
                out.push(c);
            }
        }
    }
    out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
    out
}

/// Recompute a certificate's seal and report whether it still matches —
/// tamper check for the register UI.
pub fn certificate_seal_valid(cert: &DeletionCertificate) -> bool {
    cert.seal == blake3_hex(cert.seal_input().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn build_docx(p: &str) -> Vec<u8> {
        let buf = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(buf);
        let opts = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (n, c) in [
            ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#),
            ("_rels/.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#),
        ] {
            zw.start_file(n, opts).unwrap();
            zw.write_all(c.as_bytes()).unwrap();
        }
        let body: String = (0..20).map(|i| format!("<w:p><w:r><w:t>{} {}</w:t></w:r></w:p>", p, i)).collect();
        let doc = format!(r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{}</w:body></w:document>"#, body);
        zw.start_file("word/document.xml", opts).unwrap();
        zw.write_all(doc.as_bytes()).unwrap();
        zw.finish().unwrap().into_inner()
    }

    #[test]
    fn ingest_docx_legal_then_restore_byte_exact() {
        let p = std::env::temp_dir().join("vaultops_ingest.said").to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&p);
        let mut brain = SaidFile::create(&p);
        let docx = build_docx("borrowed ingest");
        let doc_id = ingest_docx(&mut brain, &docx, "test.docx", true, "wasm@local").expect("ingest");
        let restored = restore_bytes(&mut brain, &doc_id).expect("restore");
        assert_eq!(restored, docx, "legal restore must be byte-exact");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn delete_document_erases_and_certifies() {
        let p = std::env::temp_dir().join("vaultops_del.said").to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&p);
        let mut brain = SaidFile::create(&p);
        let docx = build_docx("to be erased");
        let doc_id = ingest_docx(&mut brain, &docx, "erase.docx", true, "wasm@local").expect("ingest");

        let cert = delete_document(&mut brain, &doc_id, "alice", "GDPR request").expect("delete");
        assert_eq!(cert.filename, "erase.docx");
        assert!(!cert.seal.is_empty());
        assert!(cert.tombstone_removed);
        // Manifest gone → restore now fails.
        assert!(restore_bytes(&mut brain, &doc_id).is_err());
        // Seal verifies (recomputing over canonical fields matches).
        assert_eq!(cert.seal, crate::hasher::blake3_hex(cert.seal_input().as_bytes()));
        // Certificate is RETAINED in the erasure register (proof kept, content gone).
        let reg = list_certificates(&mut brain);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg[0].doc_id, doc_id);
        assert!(certificate_seal_valid(&reg[0]));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn delete_refused_under_legal_hold() {
        let p = std::env::temp_dir().join("vaultops_delhold.said").to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&p);
        let mut brain = SaidFile::create(&p);
        let docx = build_docx("held doc");
        let doc_id = ingest_docx(&mut brain, &docx, "held.docx", true, "wasm@local").expect("ingest");
        crate::retention::set_hold(&mut brain, &doc_id, true, "litigation");

        let err = delete_document(&mut brain, &doc_id, "alice", "try erase").unwrap_err();
        assert!(err.contains("legal hold"), "got: {}", err);
        // Document survives.
        assert!(restore_bytes(&mut brain, &doc_id).is_ok());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn ingest_docx_slim_rebuild_has_paragraphs() {
        let p = std::env::temp_dir().join("vaultops_slim.said").to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&p);
        let mut brain = SaidFile::create(&p);
        let docx = build_docx("slim rebuild");
        let doc_id = ingest_docx(&mut brain, &docx, "slim.docx", false, "wasm@local").expect("ingest slim");
        let rebuilt = rebuild_bytes(&mut brain, &doc_id).expect("rebuild");
        // rebuild produces a valid docx that re-parses with the original paragraphs
        let parsed = crate::parser::docx::parse(&rebuilt).expect("parse rebuilt");
        assert!(parsed.paragraphs.iter().any(|p| p.text.contains("slim rebuild")));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn put_object_dedups_by_doc_id() {
        let tmp = std::env::temp_dir().join("vaultops_test.said");
        let p = tmp.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&p);
        let mut brain = SaidFile::create(&p);
        put_object(&mut brain, "abc", "paragraph", b"hello").unwrap();
        put_object(&mut brain, "abc", "paragraph", b"hello").unwrap(); // dup, no-op
        let count = brain.frames.active_doc_ids().iter()
            .filter(|d| ***d == *"vault:para:abc").count();
        assert_eq!(count, 1);
        let _ = std::fs::remove_file(&p);
    }
}
