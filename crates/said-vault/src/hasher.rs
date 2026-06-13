//! BLAKE3 hashing + text canonicalization.
//!
//! Mirrors the role of vault-rust's hasher but switches the underlying
//! cryptographic primitive from SHA-256 to BLAKE3 (~10x faster, same
//! security properties). The single exception is `export-sql` (a future
//! task), which must re-hash with SHA-256 to conform to the legacy schema.

use unicode_normalization::UnicodeNormalization;

/// BLAKE3 of raw bytes, hex-encoded (64 lowercase hex chars).
pub fn blake3_hex(data: &[u8]) -> String {
    let h = blake3::hash(data);
    hex::encode(h.as_bytes())
}

/// Canonicalize text the same way the vault hashes it: NFC normalize,
/// then collapse all whitespace runs to single spaces, then trim ends.
/// Two strings that round-trip to the same canonical form will produce
/// the same hash and therefore the same dedup slot.
pub fn canonicalize_text(s: &str) -> String {
    let nfc: String = s.nfc().collect();
    nfc.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Hash a paragraph by combining canonical text with a formatting
/// fingerprint (e.g. "bold,italic" or "plain"). Different formatting
/// produces different hashes even for identical text — preserves
/// visual distinction in dedup.
pub fn hash_paragraph(text: &str, fingerprint: &str) -> String {
    let canonical = canonicalize_text(text);
    let combined = format!("{}\x00{}", canonical, fingerprint);
    blake3_hex(combined.as_bytes())
}

/// Strip a PDF font subset prefix like "ABCDEF+" from a font name.
/// Required pattern: exactly 6 uppercase ASCII letters followed by '+'.
/// Lowercase or differently-sized prefixes are NOT stripped — those
/// aren't real subset prefixes.
pub fn strip_font_prefix(name: &str) -> &str {
    let b = name.as_bytes();
    if b.len() > 7 && b[6] == b'+' && b[..6].iter().all(|c| c.is_ascii_uppercase()) {
        &name[7..]
    } else {
        name
    }
}
