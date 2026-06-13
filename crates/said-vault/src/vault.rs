//! SaidVault — orchestrates ingest, rebuild, restore, list, stats
//! against a SaidStore. Authentication is handled at the public API
//! boundary; sub-modules don't see identity context.
//!
//! Task 13 implements: init (bootstrap admin), ingest, load_manifest, stats.
//! Tasks 14-15 add: open_as (auth check), rebuild, restore.

use std::path::Path;

use sca_core::frames::Pillar;

use crate::hasher::blake3_hex;
use crate::manifest::{Manifest, ZipEntry};
use crate::parser;
use crate::rebuild::{self, RebuildRef};
use crate::store::{SaidStore, Stats};

pub struct SaidVault {
    store: SaidStore,
}

impl SaidVault {
    /// Initialize a new vault file with an admin user. Writes a bootstrap
    /// `vault:role:admin` frame (allows = ["*"], all operations) plus a
    /// `vault:user:<admin>` assignment so subsequent grant/revoke
    /// operations have something to anchor on. Saves the file to disk.
    pub fn init(path: &str, admin_user: &str) -> Result<Self, String> {
        let store = SaidStore::create(path);
        let mut v = Self { store };
        v.write_bootstrap_admin(admin_user)?;
        v.store.brain_mut().save().map_err(|e| format!("save bootstrap: {}", e))?;
        Ok(v)
    }

    /// Open an existing vault file. No identity check here — that lands in
    /// Task 14 as `open_as(path, user)`.
    pub fn open(path: &str) -> Result<Self, String> {
        let store = SaidStore::open(path)?;
        Ok(Self { store })
    }

    /// Open + authenticate as a specific user. The user must have a
    /// `vault:user:<id>` assignment frame. Returns the vault plus the
    /// user's resolved roles for subsequent authorize_tag_access checks.
    pub fn open_as(path: &str, user: &str) -> Result<(Self, Vec<crate::access::Role>), String> {
        let mut v = Self::open(path)?;
        let roles = v.load_user_roles(user)?;
        Ok((v, roles))
    }

    /// Authorize `operation` on a specific doc for a set of resolved roles.
    /// Loads the doc's manifest tags and checks them via
    /// `authorize_tag_access`. Returns a clear permission-denied error string
    /// when the user's roles don't permit the operation on this doc's tags.
    /// Used by the CLI read paths (rebuild/restore/compare) to gate every doc
    /// access through the user's role allows/denies.
    pub fn authorize_doc(
        &mut self,
        roles: &[crate::access::Role],
        doc_id: &str,
        operation: &str,
    ) -> Result<(), String> {
        let manifest = self.load_manifest(doc_id)?;
        if crate::access::authorize_tag_access(roles, &manifest.tags, operation) {
            Ok(())
        } else {
            Err(format!(
                "permission denied: your roles do not allow '{}' on doc {} (tags: {:?})",
                operation, doc_id, manifest.tags,
            ))
        }
    }

    /// True iff `operation` is permitted by any of the roles, independent of a
    /// specific doc's tags. Used to gate corpus-wide commands (list/stats):
    /// the role must list the operation and allow at least something. For
    /// per-doc filtering (which docs in a list a user may see), use
    /// `authorize_doc` per manifest.
    pub fn authorize_operation(roles: &[crate::access::Role], operation: &str) -> bool {
        // A role permits a corpus-wide read if it lists the operation AND has
        // any allow entry (a role with no allows can't see anything).
        roles.iter().any(|r| {
            r.operations.iter().any(|op| op == operation) && !r.allows.is_empty()
        })
    }

    // TODO: takes &mut self only because brain_mut().read() requires it.
    //       Conceptually read-only; would be &self if SaidFile::read were &self.
    fn load_user_roles(&mut self, user: &str) -> Result<Vec<crate::access::Role>, String> {
        let assn_doc_id = format!("vault:user:{}", user);
        let assn_json = self.store.brain_mut().read(&assn_doc_id)
            .ok_or_else(|| format!("user '{}' has no assignment frame", user))?;
        let assn: crate::access::UserAssignment = serde_json::from_str(&assn_json)
            .map_err(|e| format!("user assignment parse: {}", e))?;
        let mut roles = Vec::new();
        for role_name in &assn.roles {
            let role_doc_id = format!("vault:role:{}", role_name);
            let role_json = self.store.brain_mut().read(&role_doc_id)
                .ok_or_else(|| format!("role '{}' not found", role_name))?;
            let role: crate::access::Role = serde_json::from_str(&role_json)
                .map_err(|e| format!("role parse: {}", e))?;
            roles.push(role);
        }
        Ok(roles)
    }

    fn write_bootstrap_admin(&mut self, admin_user: &str) -> Result<(), String> {
        let admin_role = crate::access::Role {
            role: "admin".into(),
            allows: vec!["*".into()],
            denies: vec![],
            operations: crate::access::VAULT_OPERATIONS.iter().map(|s| s.to_string()).collect(),
        };
        let role_json = serde_json::to_string(&admin_role)
            .map_err(|e| format!("admin role serialize: {}", e))?;
        let role_doc_id = "vault:role:admin".to_string();
        self.store.brain_mut().remember_with_pillar(
            Some(&role_doc_id),
            &role_json,
            None,
            Pillar::Document,
            vec!["vault:role:admin".into()],
        );

        let assignment = crate::access::UserAssignment {
            user: admin_user.into(),
            roles: vec!["admin".into()],
            granted_at: chrono::Utc::now().to_rfc3339(),
            granted_by: "system".into(),
        };
        let assn_json = serde_json::to_string(&assignment)
            .map_err(|e| format!("admin assignment serialize: {}", e))?;
        let assn_doc_id = format!("vault:user:{}", admin_user);
        self.store.brain_mut().remember_with_pillar(
            Some(&assn_doc_id),
            &assn_json,
            None,
            Pillar::Document,
            vec![
                format!("vault:user:{}", admin_user),
                "vault:role:admin".into(),
            ],
        );
        Ok(())
    }

    /// Ingest a document. Returns the doc_id (BLAKE3 of file bytes).
    /// `keep_tombstone = true` is enterprise-tier; false is slim mode
    /// (no byte-exact restore available).
    /// `ingested_by` is the authenticated identity recorded in the manifest.
    pub fn ingest(
        &mut self,
        path: &str,
        tags: &[String],
        keep_tombstone: bool,
        ingested_by: &str,
    ) -> Result<String, String> {
        let doc_id = self.ingest_no_save(path, tags, keep_tombstone, ingested_by)?;
        self.save_compacted()?;
        Ok(doc_id)
    }

    /// Ingest many documents, then compact + save exactly once. This is the
    /// fast path for bulk loads: parsing + dedup happens per-doc in memory,
    /// but the expensive block-compaction and file write run a single time at
    /// the end rather than once per document. Returns one doc_id per input
    /// path in order (idempotent inputs return their existing doc_id).
    pub fn ingest_batch(
        &mut self,
        paths: &[String],
        tags: &[String],
        keep_tombstone: bool,
        ingested_by: &str,
    ) -> Result<Vec<String>, String> {
        let mut doc_ids = Vec::with_capacity(paths.len());
        for path in paths {
            doc_ids.push(self.ingest_no_save(path, tags, keep_tombstone, ingested_by)?);
        }
        self.save_compacted()?;
        Ok(doc_ids)
    }

    /// Compact the vault's frame table into block-compressed (ZstdDictBlock)
    /// form with a trained dictionary, then write the file. This is the
    /// vault-specific durable-write path — it does NOT change the generic
    /// `SaidFile::save`, so personal brains are unaffected. Vault asset frames
    /// are deduped binary blobs that compress poorly one-at-a-time but ~70%
    /// as a block (H.265 GOP approach); this is the step that delivers the
    /// spec's storage savings vs raw + SQLite.
    pub fn save_compacted(&mut self) -> Result<(), String> {
        let brain = self.store.brain_mut();
        brain.compact();
        brain.save().map_err(|e| format!("save: {}", e))
    }

    /// Parse + dedup-store a single document WITHOUT saving. Shared by `ingest`
    /// (which saves after) and `ingest_batch` (which saves once after all).
    fn ingest_no_save(
        &mut self,
        path: &str,
        tags: &[String],
        keep_tombstone: bool,
        ingested_by: &str,
    ) -> Result<String, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("read {}: {}", path, e))?;
        let doc_id = blake3_hex(&bytes);
        let manifest_doc_id = format!("vault:manifest:{}", doc_id);

        // Idempotency: if manifest already exists, return doc_id (no second write)
        if self.store.brain_mut().read(&manifest_doc_id).is_some() {
            return Ok(doc_id);
        }

        let format = detect_format(path)?;
        let filename = Path::new(path).file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut zip_entries: Vec<ZipEntry> = Vec::new();

        match format.as_str() {
            "docx" => {
                let parsed = parser::docx::parse(&bytes).map_err(|e| e.to_string())?;
                for p in &parsed.paragraphs {
                    self.store.put_object(&p.hash, "paragraph", p.text.as_bytes())?;
                    zip_entries.push(ZipEntry {
                        name: "paragraph".into(),
                        asset_hash: p.hash.clone(),
                        kind: "paragraph".into(),
                    });
                }
                for img in &parsed.images {
                    self.store.put_object(&img.hash, "image", &img.data)?;
                    zip_entries.push(ZipEntry {
                        name: img.name.clone(),
                        asset_hash: img.hash.clone(),
                        kind: "image".into(),
                    });
                }
                for f in &parsed.fonts {
                    self.store.put_object(&f.hash, "font", &f.data)?;
                    zip_entries.push(ZipEntry {
                        name: f.name.clone(),
                        asset_hash: f.hash.clone(),
                        kind: "font".into(),
                    });
                }
                for x in &parsed.structural_xml {
                    self.store.put_object(&x.hash, "xml", &x.data)?;
                    zip_entries.push(ZipEntry {
                        name: x.name.clone(),
                        asset_hash: x.hash.clone(),
                        kind: "xml".into(),
                    });
                }
            }
            "pdf" => {
                // For v1, ingest PDFs without password support.
                let parsed = parser::pdf::parse(&bytes, "")
                    .map_err(|e| format!("pdf parse: {:?}", e))?;
                for p in &parsed.paragraphs {
                    self.store.put_object(&p.hash, "paragraph", p.text.as_bytes())?;
                    zip_entries.push(ZipEntry {
                        name: "paragraph".into(),
                        asset_hash: p.hash.clone(),
                        kind: "paragraph".into(),
                    });
                }
                for img in &parsed.images {
                    self.store.put_object(&img.hash, "image", &img.data)?;
                    zip_entries.push(ZipEntry {
                        name: img.name.clone(),
                        asset_hash: img.hash.clone(),
                        kind: "image".into(),
                    });
                }
                for f in &parsed.fonts {
                    self.store.put_object(&f.hash, "font", &f.data)?;
                    zip_entries.push(ZipEntry {
                        name: f.name.clone(),
                        asset_hash: f.hash.clone(),
                        kind: "font".into(),
                    });
                }
            }
            _ => return Err(format!("unsupported format: {}", format)),
        }

        let tombstone_hash = if keep_tombstone {
            // Build the stored payload: a 1-byte marker (0x01 recipe / 0x00
            // verbatim) + the body. Use a preflate recipe ONLY if it decodes
            // back to the exact original bytes (whole-file double-verify);
            // otherwise store the original verbatim. Never silently lossy.
            let mut payload = Vec::with_capacity(bytes.len() + 1);
            match crate::preflate_recipe::encode(&bytes) {
                Some(recipe) if crate::preflate_recipe::decode(&recipe).ok().as_deref() == Some(&bytes[..]) => {
                    payload.push(0x01);
                    payload.extend_from_slice(&recipe);
                }
                _ => {
                    payload.push(0x00);
                    payload.extend_from_slice(&bytes);
                }
            }
            self.store.brain_mut().vault_tombstones_mut().put(&doc_id, &payload);
            // Manifest hash is BLAKE3 of the ORIGINAL file, not the payload.
            Some(crate::hasher::blake3_hex(&bytes))
        } else {
            None
        };

        let manifest = Manifest {
            doc_id: doc_id.clone(),
            format: format.clone(),
            filename,
            size_bytes: bytes.len() as u64,
            ingested_at: chrono::Utc::now().to_rfc3339(),
            ingested_by: ingested_by.to_string(),
            tags: tags.to_vec(),
            zip_entries,
            tombstone_hash,
        };

        let manifest_json = manifest.to_json();
        let mut manifest_tags = vec![
            "vault:manifest".into(),
            format!("vault:fmt:{}", manifest.format),
            format!("vault:filename:{}", manifest.filename),
        ];
        manifest_tags.extend(manifest.tags.iter().cloned());

        self.store.brain_mut().remember_with_pillar(
            Some(&manifest_doc_id),
            &manifest_json,
            None,
            Pillar::Document,
            manifest_tags,
        );

        Ok(doc_id)
    }

    /// Load the manifest for a given doc_id. Returns Err if missing or
    /// if the JSON content fails to parse.
    pub fn load_manifest(&mut self, doc_id: &str) -> Result<Manifest, String> {
        let manifest_doc_id = format!("vault:manifest:{}", doc_id);
        let json = self.store.brain_mut().read(&manifest_doc_id)
            .ok_or_else(|| format!("no manifest for doc_id {}", doc_id))?;
        Manifest::from_json(&json).map_err(|e| format!("manifest parse: {}", e))
    }

    /// Aggregate asset statistics from the underlying SaidStore.
    pub fn stats(&self) -> Result<Stats, String> {
        self.store.stats()
    }

    /// Rebuild the document from its dedup parts, returning the bytes in
    /// memory. Available to both slim and enterprise tiers. Output is
    /// functionally equivalent to the original but NOT necessarily byte-exact
    /// (use `restore` for byte-exact recovery).
    pub fn rebuild_bytes(&mut self, doc_id: &str) -> Result<Vec<u8>, String> {
        let manifest = self.load_manifest(doc_id)?;
        let mut refs = Vec::new();
        for entry in &manifest.zip_entries {
            let bytes = self.store.get_object_kinded(&entry.kind, &entry.asset_hash)?
                .ok_or_else(|| format!("missing {} asset {} for doc {}", entry.kind, entry.asset_hash, doc_id))?;
            refs.push(RebuildRef {
                kind: entry.kind.clone(),
                name: Some(entry.name.clone()),
                bytes,
            });
        }
        match manifest.format.as_str() {
            "docx" => rebuild::docx::rebuild(&refs),
            "pdf" => rebuild::pdf::rebuild(&refs),
            other => Err(format!("unsupported format for rebuild: {}", other)),
        }
    }

    /// Rebuild the document from its dedup parts. Writes to <out_dir>/<filename>.
    /// Thin disk-writing wrapper over `rebuild_bytes`.
    pub fn rebuild(&mut self, doc_id: &str, out_dir: &str) -> Result<String, String> {
        let manifest = self.load_manifest(doc_id)?;
        let bytes = self.rebuild_bytes(doc_id)?;
        let out_path = std::path::Path::new(out_dir).join(&manifest.filename);
        std::fs::write(&out_path, &bytes).map_err(|e| format!("write: {}", e))?;
        Ok(out_path.to_string_lossy().to_string())
    }

    /// Restore the byte-exact original from the tombstone section, returning
    /// the bytes in memory. Errors clearly when the vault was ingested in slim
    /// mode (no tombstone). Re-verifies BLAKE3 against the manifest record.
    pub fn restore_bytes(&mut self, doc_id: &str) -> Result<Vec<u8>, String> {
        let manifest = self.load_manifest(doc_id)?;
        let expected_hash = manifest.tombstone_hash.as_ref().ok_or_else(|| format!(
            "no tombstone for doc {} — vault was ingested in slim mode (--no-tombstone). \
             Use `rebuild` instead for parts-based reconstruction.",
            doc_id,
        ))?.clone();
        let payload = self.store.brain_mut().vault_tombstones_mut().get(doc_id)
            .ok_or_else(|| format!("tombstone for {} missing or BLAKE3-mismatched", doc_id))?;
        if payload.is_empty() {
            return Err(format!("tombstone for {} is empty", doc_id));
        }
        // Marker byte: 0x01 = preflate recipe, 0x00 = verbatim whole-file.
        let bytes = match payload[0] {
            0x01 => crate::preflate_recipe::decode(&payload[1..])
                .map_err(|e| format!("recipe decode for {}: {}", doc_id, e))?,
            0x00 => payload[1..].to_vec(),
            other => return Err(format!("unknown tombstone marker {} for {}", other, doc_id)),
        };
        let actual_hash = crate::hasher::blake3_hex(&bytes);
        if actual_hash != expected_hash {
            return Err(format!(
                "tombstone BLAKE3 mismatch for {}: expected {} got {}",
                doc_id, expected_hash, actual_hash,
            ));
        }
        Ok(bytes)
    }

    /// Restore the byte-exact original from the tombstone section. Writes to
    /// <out_dir>/<filename>. Thin disk-writing wrapper over `restore_bytes`.
    pub fn restore(&mut self, doc_id: &str, out_dir: &str) -> Result<String, String> {
        let manifest = self.load_manifest(doc_id)?;
        let bytes = self.restore_bytes(doc_id)?;
        let out_path = std::path::Path::new(out_dir).join(&manifest.filename);
        std::fs::write(&out_path, &bytes).map_err(|e| format!("write: {}", e))?;
        Ok(out_path.to_string_lossy().to_string())
    }

    /// Compare the byte-exact restore against the parts-based rebuild for a
    /// doc. Enterprise tier only (needs a tombstone). Restores + rebuilds both
    /// to memory, parses each to paragraph lists, and reports byte counts,
    /// paragraph counts, text-match, and byte-identical (expected false by
    /// design — rebuild re-zips, so byte layout differs even when content
    /// matches). Mirrors the vault-rust `compare` semantics.
    pub fn compare(&mut self, doc_id: &str) -> Result<CompareReport, String> {
        let restored = self.restore_bytes(doc_id)?;
        let rebuilt = self.rebuild_bytes(doc_id)?;
        let format = self.load_manifest(doc_id)?.format;

        let restored_paras = parse_paragraphs(&format, &restored)?;
        let rebuilt_paras = parse_paragraphs(&format, &rebuilt)?;

        let text_match = restored_paras == rebuilt_paras;
        let mut first_diffs = Vec::new();
        if !text_match {
            let max = restored_paras.len().max(rebuilt_paras.len());
            for i in 0..max {
                let a = restored_paras.get(i).map(|s| s.as_str()).unwrap_or("<none>");
                let b = rebuilt_paras.get(i).map(|s| s.as_str()).unwrap_or("<none>");
                if a != b {
                    first_diffs.push((i, a.to_string(), b.to_string()));
                    if first_diffs.len() >= 5 { break; }
                }
            }
        }

        Ok(CompareReport {
            doc_id: doc_id.to_string(),
            restored_bytes: restored.len(),
            rebuilt_bytes: rebuilt.len(),
            restored_paragraphs: restored_paras.len(),
            rebuilt_paragraphs: rebuilt_paras.len(),
            text_match,
            byte_identical: restored == rebuilt,
            first_diffs,
        })
    }

    pub fn store(&self) -> &SaidStore { &self.store }
    pub fn store_mut(&mut self) -> &mut SaidStore { &mut self.store }
}

fn detect_format(path: &str) -> Result<String, String> {
    let ext = Path::new(path).extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| format!("no extension in path: {}", path))?
        .to_lowercase();
    match ext.as_str() {
        "docx" | "pdf" => Ok(ext),
        other => Err(format!("unsupported format: {}", other)),
    }
}

/// Parse a document's bytes to its ordered paragraph text list, using the
/// format-specific parser. Shared by `compare`.
fn parse_paragraphs(format: &str, bytes: &[u8]) -> Result<Vec<String>, String> {
    match format {
        "docx" => Ok(parser::docx::parse(bytes).map_err(|e| e.to_string())?
            .paragraphs.into_iter().map(|p| p.text).collect()),
        "pdf" => Ok(parser::pdf::parse(bytes, "").map_err(|e| format!("pdf parse: {:?}", e))?
            .paragraphs.into_iter().map(|p| p.text).collect()),
        other => Err(format!("unsupported format for compare: {}", other)),
    }
}

/// Result of a `compare` — byte-exact restore vs parts-based rebuild.
#[derive(Debug, Clone)]
pub struct CompareReport {
    pub doc_id: String,
    pub restored_bytes: usize,
    pub rebuilt_bytes: usize,
    pub restored_paragraphs: usize,
    pub rebuilt_paragraphs: usize,
    /// True when restore and rebuild yield the same ordered paragraph text.
    pub text_match: bool,
    /// True only if the two byte streams are identical. Expected FALSE by
    /// design: rebuild re-zips, so byte layout differs even when text matches.
    pub byte_identical: bool,
    /// Up to 5 (index, restored_text, rebuilt_text) tuples where they differ.
    pub first_diffs: Vec<(usize, String, String)>,
}
