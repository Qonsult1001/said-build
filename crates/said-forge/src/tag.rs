//! `forge:<type>:<short-hash>:<slug>` tag helpers.
//!
//! Per spec §6.1: `<short-hash>` is the first 5 lowercase hex chars of
//! `blake3(directive_source + 'v1')`. Slugs are kebab-case, max 64 chars.

use blake3::Hasher;

/// Compute the stable 5-hex-char directive hash.
///
/// The hash is derived from the directive *source identifier* (path or URL)
/// plus a version salt ('v1') so the namespace is portable across machines
/// yet evolvable if we ever need to bust hashes.
pub fn forge_hash(directive_source: &str) -> String {
    let mut hasher = Hasher::new();
    hasher.update(directive_source.as_bytes());
    hasher.update(b"v1");
    let full = hasher.finalize();
    let hex = full.to_hex();
    // first 5 hex chars (2.5 bytes' worth of entropy = ~1M collision-free
    // directive/story pairs, plenty for any real batch).
    hex[..5].to_string()
}

/// Sanitize an arbitrary identifier into a kebab-case slug, max 64 chars.
///
/// Rules (stable across releases — frame IDs depend on these):
/// - lowercase
/// - runs of non-[a-z0-9] replaced with a single `-`
/// - leading/trailing dashes trimmed
/// - truncated to 64 chars
/// - empty input becomes `"unnamed"`
pub fn sanitize_slug(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let mut out = String::with_capacity(raw.len());
    let mut last_was_dash = true; // so leading non-[a-z0-9] is absorbed
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_dash = false;
        } else if !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }
    // trim trailing dash
    while out.ends_with('-') {
        out.pop();
    }
    // truncate to 64 chars
    if out.len() > 64 {
        out.truncate(64);
        while out.ends_with('-') {
            out.pop();
        }
    }
    if out.is_empty() {
        "unnamed".to_string()
    } else {
        out
    }
}

/// Build a fully-qualified forge tag.
///
/// Examples:
/// - `forge_tag("story", "a3f91", "post-accounts", None, None)` →
///   `"forge:story:a3f91:post-accounts"`
/// - `forge_tag("run", "a3f91", "post-accounts", Some(1), Some("input"))` →
///   `"forge:run:a3f91:post-accounts:r1:input"`
pub fn forge_tag(
    ty: &str,
    hash: &str,
    slug: &str,
    run_n: Option<u32>,
    part: Option<&str>,
) -> String {
    let mut out = format!("forge:{}:{}:{}", ty, hash, slug);
    if let Some(n) = run_n {
        out.push_str(&format!(":r{}", n));
    }
    if let Some(p) = part {
        out.push(':');
        out.push_str(p);
    }
    out
}

/// Inverse of `forge_tag`: parse a forge tag back into its components.
///
/// Returns `None` if the tag doesn't begin with `forge:` or has fewer than
/// 4 colon-separated components.
pub fn parse_forge_tag(tag: &str) -> Option<ForgeTagParts> {
    let mut parts = tag.split(':');
    if parts.next()? != "forge" {
        return None;
    }
    let ty = parts.next()?.to_string();
    let hash = parts.next()?.to_string();
    let slug = parts.next()?.to_string();
    let mut run_n = None;
    let mut part = None;
    for remaining in parts {
        if let Some(stripped) = remaining.strip_prefix('r') {
            if let Ok(n) = stripped.parse::<u32>() {
                run_n = Some(n);
                continue;
            }
        }
        part = Some(remaining.to_string());
    }
    Some(ForgeTagParts { ty, hash, slug, run_n, part })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeTagParts {
    pub ty: String,
    pub hash: String,
    pub slug: String,
    pub run_n: Option<u32>,
    pub part: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_hash_is_deterministic() {
        assert_eq!(forge_hash("fixtures/petstore.yaml"), forge_hash("fixtures/petstore.yaml"));
    }

    #[test]
    fn forge_hash_differs_for_different_inputs() {
        let a = forge_hash("petstore.yaml");
        let b = forge_hash("requirements.md");
        assert_ne!(a, b);
    }

    #[test]
    fn forge_hash_is_5_lowercase_hex() {
        let h = forge_hash("anything");
        assert_eq!(h.len(), 5);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit() && (!c.is_alphabetic() || c.is_lowercase())));
    }

    #[test]
    fn sanitize_slug_basic_cases() {
        assert_eq!(sanitize_slug("Create Account"), "create-account");
        assert_eq!(sanitize_slug("POST /accounts/{id}"), "post-accounts-id");
        assert_eq!(sanitize_slug("  ---leading---"), "leading");
        assert_eq!(sanitize_slug("trailing---"), "trailing");
        assert_eq!(sanitize_slug("already-kebab-case"), "already-kebab-case");
    }

    #[test]
    fn sanitize_slug_truncates_to_64() {
        let long = "a".repeat(200);
        let s = sanitize_slug(&long);
        assert_eq!(s.len(), 64);
    }

    #[test]
    fn sanitize_slug_empty_becomes_unnamed() {
        assert_eq!(sanitize_slug(""), "unnamed");
        assert_eq!(sanitize_slug("!!!"), "unnamed");
    }

    #[test]
    fn forge_tag_no_run_no_part() {
        assert_eq!(
            forge_tag("story", "a3f91", "post-accounts", None, None),
            "forge:story:a3f91:post-accounts"
        );
    }

    #[test]
    fn forge_tag_with_run_and_part() {
        assert_eq!(
            forge_tag("run", "a3f91", "post-accounts", Some(1), Some("input")),
            "forge:run:a3f91:post-accounts:r1:input"
        );
    }

    #[test]
    fn parse_forge_tag_roundtrips() {
        let original = "forge:run:a3f91:post-accounts:r1:input";
        let parts = parse_forge_tag(original).unwrap();
        assert_eq!(parts.ty, "run");
        assert_eq!(parts.hash, "a3f91");
        assert_eq!(parts.slug, "post-accounts");
        assert_eq!(parts.run_n, Some(1));
        assert_eq!(parts.part.as_deref(), Some("input"));
        assert_eq!(
            forge_tag(&parts.ty, &parts.hash, &parts.slug, parts.run_n, parts.part.as_deref()),
            original
        );
    }

    #[test]
    fn parse_forge_tag_rejects_non_forge() {
        assert!(parse_forge_tag("ingest:sql:something").is_none());
        assert!(parse_forge_tag("not:enough").is_none());
    }
}
