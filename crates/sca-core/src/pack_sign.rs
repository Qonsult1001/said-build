//! Skill-pack signing & verification — Ed25519 publisher provenance (Gate 3).
//!
//! This is the TRUST gate for `.said` skill packs (and any published/sold `.said`):
//! it proves a pack was published by an APPROVED publisher and has NOT been tampered
//! with since signing. It is the cryptographic half of
//! [docs/said-structure/18-skill-pack-linking-and-trust.md] (Gate 3), aligned with the
//! Hub spec [docs/said-structure/05-features/row-52-hub.md] §1.2.
//!
//! ## ⚠️ OUTSTANDING FOR LAUNCH — merge the signature INTO the `.said` file
//!
//! **This module signs DETACHED: it writes a sidecar `<pack>.said.sig` next to the
//! pack.** That ships the exact crypto + trust model with ZERO change to the v7_1 file
//! format (lowest risk, fastest). **Before launch this MUST be folded into an in-file
//! `SIGN` section** (row-52 §1.2: 16-byte magic + 64-byte sig + 32-byte pubkey + 4-byte
//! alg, appended to the file with a header slot) so a pack is a single self-describing
//! file with no sidecar to lose or strip. The verification math here (BLAKE3 content
//! root over the pack bytes → Ed25519 verify → allowlist check) is identical either way;
//! only WHERE the 100-byte signature lives changes. Tracked in
//! [docs/said-structure/12-roadmap.md] and doc 18 "Outstanding for launch".
//!
//! ## What is signed
//!
//! The **content root** = `BLAKE3(pack file bytes)`. For the detached sidecar we hash the
//! whole `.said` file as it sits on disk (it is immutable once published — `BrainMode`
//! Locked). When this moves in-file, the root becomes `BLAKE3(FTOC || DICT || BLKT ||
//! data_section)` per row-52 (i.e. everything EXCEPT the SIGN section itself), which is
//! the same guarantee computed over the structured sections instead of raw bytes.
//!
//! ## The sidecar format (`<pack>.said.sig`, 116 bytes, binary)
//!
//! ```text
//! [0..16]    magic  b"SAIDSIG\0\0\0\0\0\0\0\0\0"  (16 bytes, version/identify)
//! [16..80]   sig    64-byte Ed25519 signature over the 32-byte content root
//! [80..112]  pubkey 32-byte Ed25519 public key of the publisher
//! [112..116] alg    u32 LE = 1 (Ed25519 over BLAKE3-256 content root)
//! ```
//!
//! ## Verify-at-mount flow (consumer side)
//! 1. Read the sidecar; reject if missing/short/bad magic.
//! 2. Recompute the content root over the current pack bytes.
//! 3. Ed25519-verify the signature with the embedded pubkey (offline; no network).
//! 4. Cross-reference the pubkey against the local APPROVED-PUBLISHER allowlist
//!    (`~/.said/publishers/*.pub`, cached from the registry — the trust root).
//! Pass → safe to mount. Any failure → refuse + log (never mount in strict mode).
//!
//! Why both (3) and (4): step 3 proves *some* key signed it and it's untampered; step 4
//! proves that key is one we TRUST (the official publisher), not just any key.

use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, Verifier, Signature, SigningKey, VerifyingKey};

/// Sidecar magic — identifies a SAID detached signature file + leaves room to version.
pub const SIG_MAGIC: &[u8; 16] = b"SAIDSIG\0\0\0\0\0\0\0\0\0";
/// Signature algorithm id stored in the sidecar (Ed25519 over BLAKE3-256 content root).
pub const SIG_ALG_ED25519_BLAKE3: u32 = 1;
const SIDECAR_LEN: usize = 16 + 64 + 32 + 4; // 116

/// The content root that gets signed: BLAKE3-256 over the pack file's bytes.
/// (In-file SIGN section will hash the structured sections instead — same guarantee.)
pub fn content_root(pack_path: &Path) -> Result<[u8; 32], String> {
    let bytes = std::fs::read(pack_path).map_err(|e| format!("read pack {}: {}", pack_path.display(), e))?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

/// The conventional sidecar path for a pack: `<pack>.sig` (i.e. `x.said.sig`).
pub fn sidecar_path(pack_path: &Path) -> PathBuf {
    let mut s = pack_path.as_os_str().to_os_string();
    s.push(".sig");
    PathBuf::from(s)
}

/// PUBLISHER side: sign `pack_path` with `signing_key`, writing the detached sidecar.
/// Returns the sidecar path. The pack file itself is untouched (immutable once published).
pub fn sign_pack(pack_path: &Path, signing_key: &SigningKey) -> Result<PathBuf, String> {
    let root = content_root(pack_path)?;
    let sig: Signature = signing_key.sign(&root);
    let pubkey: VerifyingKey = signing_key.verifying_key();

    let mut out = Vec::with_capacity(SIDECAR_LEN);
    out.extend_from_slice(SIG_MAGIC);
    out.extend_from_slice(&sig.to_bytes());
    out.extend_from_slice(pubkey.as_bytes());
    out.extend_from_slice(&SIG_ALG_ED25519_BLAKE3.to_le_bytes());
    debug_assert_eq!(out.len(), SIDECAR_LEN);

    let path = sidecar_path(pack_path);
    std::fs::write(&path, &out).map_err(|e| format!("write sidecar {}: {}", path.display(), e))?;
    Ok(path)
}

/// Result of verifying a pack: the publisher pubkey (hex) on success.
#[derive(Debug, Clone)]
pub struct Verified {
    /// Hex of the 32-byte Ed25519 public key that signed the pack.
    pub pubkey_hex: String,
}

/// CONSUMER side, step 1-3: verify the detached signature against the current pack bytes.
/// Returns the signing pubkey (for the caller's allowlist check) or an error describing
/// WHY it failed (missing sidecar / bad magic / tamper / bad sig). Does NOT consult the
/// allowlist — that is [`is_approved`], kept separate so callers can log the distinction
/// "untrusted-but-valid-signature" vs "tampered/invalid".
pub fn verify_pack(pack_path: &Path) -> Result<Verified, String> {
    let sc = sidecar_path(pack_path);
    let raw = std::fs::read(&sc).map_err(|_| format!("no signature sidecar for {}", pack_path.display()))?;
    if raw.len() != SIDECAR_LEN {
        return Err(format!("signature sidecar {} has wrong size ({} != {})", sc.display(), raw.len(), SIDECAR_LEN));
    }
    if &raw[0..16] != SIG_MAGIC {
        return Err("signature sidecar bad magic (not a SAIDSIG file)".into());
    }
    let sig_bytes: [u8; 64] = raw[16..80].try_into().unwrap();
    let pk_bytes: [u8; 32] = raw[80..112].try_into().unwrap();
    let pubkey = VerifyingKey::from_bytes(&pk_bytes).map_err(|e| format!("bad pubkey in sidecar: {}", e))?;
    let sig = Signature::from_bytes(&sig_bytes);

    let root = content_root(pack_path)?;
    pubkey
        .verify(&root, &sig)
        .map_err(|_| "signature verification FAILED — pack tampered or wrong key".to_string())?;
    Ok(Verified { pubkey_hex: hex_lower(&pk_bytes) })
}

/// CONSUMER side, step 4: is this pubkey in the approved-publisher allowlist?
/// `allowlist` is the set of trusted pubkey hex strings (loaded from
/// `~/.said/publishers/*.pub`, cached from the registry). Empty allowlist → nothing is
/// approved (strict-by-default; the caller decides whether to allow unsigned/unknown).
pub fn is_approved(verified: &Verified, allowlist: &std::collections::HashSet<String>) -> bool {
    allowlist.contains(&verified.pubkey_hex)
}

/// Generate a fresh Ed25519 publisher keypair (publisher tooling / tests).
/// Returns (signing_key, pubkey_hex). Keep the signing key secret; publish the pubkey hex.
pub fn generate_keypair() -> (SigningKey, String) {
    let sk = SigningKey::generate(&mut rand_core::OsRng);
    let pk_hex = hex_lower(sk.verifying_key().as_bytes());
    (sk, pk_hex)
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify_roundtrip_and_tamper_detection() {
        let dir = std::env::temp_dir().join(format!("said_sign_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pack = dir.join("pack.said");
        std::fs::write(&pack, b"pretend .said pack bytes v1").unwrap();

        let (sk, pk_hex) = generate_keypair();
        let sc = sign_pack(&pack, &sk).unwrap();
        assert!(sc.exists());

        // Valid signature verifies, returns the right pubkey.
        let v = verify_pack(&pack).expect("should verify");
        assert_eq!(v.pubkey_hex, pk_hex);

        // Allowlist check.
        let mut allow = std::collections::HashSet::new();
        assert!(!is_approved(&v, &allow), "empty allowlist trusts nobody");
        allow.insert(pk_hex.clone());
        assert!(is_approved(&v, &allow), "pubkey in allowlist is approved");

        // TAMPER the pack → verification must fail (untampered guarantee).
        std::fs::write(&pack, b"pretend .said pack bytes v1 TAMPERED").unwrap();
        assert!(verify_pack(&pack).is_err(), "tampered pack must fail verification");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_sidecar_errors() {
        let dir = std::env::temp_dir().join(format!("said_sign_nosig_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pack = dir.join("unsigned.said");
        std::fs::write(&pack, b"unsigned pack").unwrap();
        assert!(verify_pack(&pack).is_err(), "no sidecar → error");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
