//! Hasher tests — ported from vault-rust + updated to BLAKE3.

use said_vault::hasher::*;

#[test]
fn canonicalize_text_normalizes_nfc_and_collapses_whitespace() {
    assert_eq!(canonicalize_text("Hello  World"), "Hello World");
    assert_eq!(canonicalize_text("  leading"), "leading");
    assert_eq!(canonicalize_text("trailing  "), "trailing");
    // Non-breaking space (U+00A0) is Unicode whitespace — split_whitespace
    // collapses it (NFC does not alter U+00A0; it has no canonical decomposition)
    assert_eq!(canonicalize_text("Hello\u{00A0}World"), "Hello World");
}

#[test]
fn blake3_hex_is_64_lowercase_hex_chars() {
    let h = blake3_hex(b"test content");
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit() && (c.is_ascii_digit() || c.is_ascii_lowercase())));
}

#[test]
fn blake3_hex_is_deterministic() {
    assert_eq!(blake3_hex(b"hello"), blake3_hex(b"hello"));
    assert_ne!(blake3_hex(b"hello"), blake3_hex(b"world"));
}

#[test]
fn hash_paragraph_is_stable_after_canonicalization() {
    // Two visually-identical-but-byte-different paragraphs hash the same
    let a = hash_paragraph("Hello World ", "plain");
    let b = hash_paragraph("Hello\u{00A0}\u{00A0}World", "plain");
    assert_eq!(a, b, "canonicalization must collapse cosmetic differences");
}

#[test]
fn hash_paragraph_distinguishes_fingerprints() {
    // Same text, different formatting fingerprint → different hash
    let plain = hash_paragraph("Hello World", "plain");
    let bold  = hash_paragraph("Hello World", "bold");
    assert_ne!(plain, bold, "formatting fingerprint must affect the hash");
}

#[test]
fn strip_font_prefix_removes_pdf_subset_prefix() {
    assert_eq!(strip_font_prefix("ABCDEF+Helvetica"), "Helvetica");
    assert_eq!(strip_font_prefix("Helvetica"), "Helvetica");
    assert_eq!(strip_font_prefix(""), "");
    // Lowercase prefix is not a real PDF subset prefix — leave alone
    assert_eq!(strip_font_prefix("abcdef+Helvetica"), "abcdef+Helvetica");
}

#[test]
fn strip_font_prefix_does_not_panic_on_multi_byte_input() {
    // "😀😀xyz" has byte length 11 (guard 'len > 7' passes) but the first
    // bytes are NOT ASCII uppercase letters. Must NOT panic — must return
    // the name unchanged. Pre-fix this case panicked at chars[6] indexing.
    assert_eq!(strip_font_prefix("😀😀xyz"), "😀😀xyz");

    // Edge: exactly 8 bytes, all in two 4-byte emoji
    assert_eq!(strip_font_prefix("😀😀"), "😀😀");

    // Edge: 6 ASCII uppercase byte-prefix followed by '+' and a multi-byte
    // suffix — the strip should succeed and the suffix should round-trip.
    assert_eq!(strip_font_prefix("ABCDEF+😀"), "😀");
}
