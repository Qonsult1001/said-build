//! Append-only audit log for `.said` brains.
//!
//! Every mutating operation (`remember`, `delete`, `forget`, admin actions)
//! writes one `AuditEntry` to an in-memory ring, chained by BLAKE3 so
//! tampering with any past entry invalidates the chain. On `save()` the
//! entries serialize into an `AUDT` section of the `.said` file; on
//! `open()` the section loads + verification walks the chain.
//!
//! This is the append-only layer that turns "admin actions" into real
//! compliance events: who did what, when, chained so nothing can be
//! silently rewritten.
//!
//! Mode semantics:
//! - Portable brains: log entries use `actor: "owner"` by default (single-
//!   user). Callers can override via `set_actor()` before the mutating call.
//! - Enterprise brains: `AppGrant` enforcement kicks in at the MCP dispatch
//!   layer — every tool call must carry an app-id header, and writes that
//!   don't have a matching grant get refused.

use std::collections::HashMap;

/// A single audit event. All fields are stable for serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// Monotonic sequence number; first entry = 0.
    pub seq: u64,
    /// Unix epoch seconds.
    pub timestamp: u64,
    /// Actor identifier (user id, app id, or "owner" / "system").
    pub actor: String,
    /// Event kind — short machine-readable tag.
    /// Known values: remember, delete, forget, restore, legal_hold_add,
    /// legal_hold_release, retention_sweep, mode_set, dream_cycle.
    pub kind: String,
    /// Target doc_id or empty for global events.
    pub target: String,
    /// Free-form detail line (e.g. "frame #142", "case=CASE-42", byte counts).
    pub detail: String,
    /// BLAKE3(prev_hash || seq || timestamp || actor || kind || target || detail)
    /// — 32 bytes.
    pub hash: [u8; 32],
}

/// In-memory audit log — owns the full chain. Serialized as `AUDT` section.
#[derive(Debug, Clone, Default)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
    /// Current actor id used when callers call `append_*` without overriding.
    /// Defaults to "owner" for Portable brains.
    current_actor: String,
}

impl AuditLog {
    pub fn new() -> Self {
        Self { entries: Vec::new(), current_actor: "owner".to_string() }
    }

    /// Override the actor tag recorded by subsequent appends. MCP dispatch
    /// sets this per-call based on the caller's app-id / session; CLI leaves
    /// it at "owner".
    pub fn set_actor(&mut self, actor: impl Into<String>) {
        self.current_actor = actor.into();
    }

    pub fn current_actor(&self) -> &str { &self.current_actor }
    pub fn entries(&self) -> &[AuditEntry] { &self.entries }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    /// Compute the chained hash for a new entry given the previous head.
    fn chain_hash(
        prev: &[u8; 32], seq: u64, ts: u64, actor: &str, kind: &str, target: &str, detail: &str,
    ) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(prev);
        hasher.update(&seq.to_le_bytes());
        hasher.update(&ts.to_le_bytes());
        hasher.update(actor.as_bytes());
        hasher.update(&[0u8]);
        hasher.update(kind.as_bytes());
        hasher.update(&[0u8]);
        hasher.update(target.as_bytes());
        hasher.update(&[0u8]);
        hasher.update(detail.as_bytes());
        *hasher.finalize().as_bytes()
    }

    /// Append a new entry using the current actor and current time. Returns
    /// the entry's `seq`.
    pub fn append(&mut self, kind: &str, target: &str, detail: &str) -> u64 {
        let seq = self.entries.len() as u64;
        let ts = crate::time_compat::unix_secs();
        let prev = self.entries.last().map(|e| e.hash).unwrap_or([0u8; 32]);
        let actor = self.current_actor.clone();
        let hash = Self::chain_hash(&prev, seq, ts, &actor, kind, target, detail);
        self.entries.push(AuditEntry {
            seq, timestamp: ts, actor, kind: kind.to_string(),
            target: target.to_string(), detail: detail.to_string(), hash,
        });
        seq
    }

    /// Walk the chain and verify every link. Returns `Ok(())` on a valid
    /// chain, or `Err` with the first seq where the chain breaks.
    pub fn verify(&self) -> Result<(), String> {
        let mut prev = [0u8; 32];
        for e in &self.entries {
            let expected = Self::chain_hash(
                &prev, e.seq, e.timestamp, &e.actor, &e.kind, &e.target, &e.detail,
            );
            if expected != e.hash {
                return Err(format!("audit chain broken at seq={}", e.seq));
            }
            prev = e.hash;
        }
        Ok(())
    }

    /// Serialize the log to bytes. Layout:
    ///   b"AUDT"                 (4 bytes magic)
    ///   u32 n_entries           (little-endian)
    ///   per entry:
    ///     u64 seq
    ///     u64 timestamp
    ///     u16 actor_len, actor bytes
    ///     u16 kind_len, kind bytes
    ///     u16 target_len, target bytes
    ///     u32 detail_len, detail bytes
    ///     32 bytes hash
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.entries.len() * 80);
        out.extend_from_slice(b"AUDT");
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for e in &self.entries {
            out.extend_from_slice(&e.seq.to_le_bytes());
            out.extend_from_slice(&e.timestamp.to_le_bytes());
            let a = e.actor.as_bytes();
            out.extend_from_slice(&(a.len() as u16).to_le_bytes()); out.extend_from_slice(a);
            let k = e.kind.as_bytes();
            out.extend_from_slice(&(k.len() as u16).to_le_bytes()); out.extend_from_slice(k);
            let t = e.target.as_bytes();
            out.extend_from_slice(&(t.len() as u16).to_le_bytes()); out.extend_from_slice(t);
            let d = e.detail.as_bytes();
            out.extend_from_slice(&(d.len() as u32).to_le_bytes()); out.extend_from_slice(d);
            out.extend_from_slice(&e.hash);
        }
        out
    }

    /// Deserialize from a byte slice starting with the `AUDT` magic. Returns
    /// `None` if magic doesn't match or the stream is malformed.
    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 || &bytes[..4] != b"AUDT" { return None; }
        let n = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
        let mut log = AuditLog::new();
        let mut pos = 8;
        for _ in 0..n {
            if pos + 16 > bytes.len() { return None; }
            let seq = u64::from_le_bytes(bytes[pos..pos+8].try_into().ok()?); pos += 8;
            let timestamp = u64::from_le_bytes(bytes[pos..pos+8].try_into().ok()?); pos += 8;

            let alen = u16::from_le_bytes(bytes[pos..pos+2].try_into().ok()?) as usize; pos += 2;
            if pos + alen > bytes.len() { return None; }
            let actor = String::from_utf8(bytes[pos..pos+alen].to_vec()).ok()?; pos += alen;

            let klen = u16::from_le_bytes(bytes[pos..pos+2].try_into().ok()?) as usize; pos += 2;
            if pos + klen > bytes.len() { return None; }
            let kind = String::from_utf8(bytes[pos..pos+klen].to_vec()).ok()?; pos += klen;

            let tlen = u16::from_le_bytes(bytes[pos..pos+2].try_into().ok()?) as usize; pos += 2;
            if pos + tlen > bytes.len() { return None; }
            let target = String::from_utf8(bytes[pos..pos+tlen].to_vec()).ok()?; pos += tlen;

            let dlen = u32::from_le_bytes(bytes[pos..pos+4].try_into().ok()?) as usize; pos += 4;
            if pos + dlen > bytes.len() { return None; }
            let detail = String::from_utf8(bytes[pos..pos+dlen].to_vec()).ok()?; pos += dlen;

            if pos + 32 > bytes.len() { return None; }
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&bytes[pos..pos+32]); pos += 32;

            log.entries.push(AuditEntry {
                seq, timestamp, actor, kind, target, detail, hash,
            });
        }
        Some(log)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// AppGrant — per-app scope enforcement used at MCP dispatch time
// ════════════════════════════════════════════════════════════════════════════

/// A per-app capability grant. `app_id` identifies the caller; `actions`
/// is the set of audit kinds the app is allowed to record. Wildcard `"*"`
/// means all actions. Grants live in-memory (registered at server start)
/// and are checked by the MCP dispatch layer before every mutating tool.
#[derive(Debug, Clone)]
pub struct AppGrant {
    pub app_id: String,
    pub actions: Vec<String>,
}

impl AppGrant {
    pub fn new(app_id: impl Into<String>, actions: Vec<String>) -> Self {
        Self { app_id: app_id.into(), actions }
    }

    pub fn allows(&self, action: &str) -> bool {
        self.actions.iter().any(|a| a == "*" || a == action)
    }
}

/// In-memory registry mapping app_id → AppGrant. Enterprise MCP servers load
/// grants from config at startup; Portable brains default to a single
/// implicit `owner` grant with `*`.
#[derive(Debug, Default, Clone)]
pub struct AppGrantRegistry {
    grants: HashMap<String, AppGrant>,
    /// When true, unknown apps are refused. Portable mode sets this false so
    /// legacy callers continue to work; Enterprise mode sets it true.
    strict: bool,
}

impl AppGrantRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn set_strict(&mut self, strict: bool) { self.strict = strict; }
    pub fn is_strict(&self) -> bool { self.strict }

    pub fn register(&mut self, grant: AppGrant) {
        self.grants.insert(grant.app_id.clone(), grant);
    }

    pub fn get(&self, app_id: &str) -> Option<&AppGrant> {
        self.grants.get(app_id)
    }

    /// Check whether `app_id` is allowed to perform `action`. Returns
    /// `Ok(())` when allowed, `Err(reason)` when refused.
    pub fn check(&self, app_id: &str, action: &str) -> Result<(), String> {
        match self.grants.get(app_id) {
            Some(g) if g.allows(action) => Ok(()),
            Some(_) => Err(format!(
                "app '{}' is registered but has no grant for action '{}'",
                app_id, action,
            )),
            None if self.strict => Err(format!(
                "app '{}' has no grant registered (strict mode)",
                app_id,
            )),
            None => Ok(()),  // Portable: unknown apps pass through.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_is_consistent() {
        let mut log = AuditLog::new();
        log.append("remember", "doc1", "frame #1");
        log.append("remember", "doc2", "frame #2");
        log.append("delete", "doc1", "user invoked");
        assert!(log.verify().is_ok());
    }

    #[test]
    fn chain_detects_tampering() {
        let mut log = AuditLog::new();
        log.append("remember", "doc1", "frame #1");
        log.append("delete", "doc1", "user invoked");
        // Tamper with an entry's kind after the fact.
        log.entries[0].kind = "forget".to_string();
        assert!(log.verify().is_err());
    }

    #[test]
    fn roundtrip_serialize() {
        let mut log = AuditLog::new();
        log.set_actor("alice");
        log.append("remember", "doc1", "frame #1");
        log.set_actor("bob");
        log.append("delete", "doc1", "cleanup");
        let bytes = log.serialize();
        let restored = AuditLog::deserialize(&bytes).expect("deserialize");
        assert_eq!(restored.len(), 2);
        assert_eq!(restored.entries()[0].actor, "alice");
        assert_eq!(restored.entries()[1].actor, "bob");
        assert!(restored.verify().is_ok());
    }

    #[test]
    fn grant_check() {
        let mut reg = AppGrantRegistry::new();
        reg.register(AppGrant::new("search-ui", vec!["remember".into(), "search".into()]));
        reg.register(AppGrant::new("admin-tool", vec!["*".into()]));
        reg.set_strict(true);

        assert!(reg.check("search-ui", "remember").is_ok());
        assert!(reg.check("search-ui", "delete").is_err());
        assert!(reg.check("admin-tool", "delete").is_ok());
        assert!(reg.check("unknown-app", "search").is_err());
    }
}
