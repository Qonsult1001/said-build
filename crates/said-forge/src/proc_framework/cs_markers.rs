//! C# marker grammar — `// [SaidFully] region=<name>` / `// [SaidEnd]`.
//!
//! Mirrors the SQL `@said-managed` marker model but uses C# line-comment
//! syntax. Same three modes (Fully / Ignore / Merge).
//!
//! Examples:
//!   `// [SaidFully] region=file-header`
//!   `    // [SaidIgnore] slot=author-content`
//!   `// [SaidMerge] region=foo`
//!   `// [SaidEnd]`
//!
//! Why a separate parser from `markers.rs`: SQL uses `-- @said-managed: Fully
//! region=X` (verbose, single keyword pair). C# uses `// [SaidFully]
//! region=X` (bracket-attribute-shaped). Different grammars; cleaner with
//! their own implementations than a switch-on-comment-style hybrid.

use super::markers::ManagedMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsRegion {
    pub mode: ManagedMode,
    pub name: String,
    pub body: String,
    pub line_start: usize,
    pub line_end: usize,
}

pub fn parse_cs_regions(text: &str) -> Result<Vec<CsRegion>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut regions = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some((mode, name)) = parse_open(lines[i]) {
            let mut j = i + 1;
            let mut found_end = false;
            while j < lines.len() {
                if is_end(lines[j]) {
                    found_end = true;
                    break;
                }
                j += 1;
            }
            if !found_end {
                return Err(format!(
                    "malformed C# marker at line {}: '{}' has no closing // [SaidEnd]",
                    i + 1,
                    lines[i].trim()
                ));
            }
            let body = if j > i + 1 {
                lines[i + 1..j].join("\n")
            } else {
                String::new()
            };
            regions.push(CsRegion {
                mode,
                name,
                body,
                line_start: i + 1,
                line_end: j + 1,
            });
            i = j + 1;
        } else {
            i += 1;
        }
    }
    Ok(regions)
}

/// Parse `// [SaidFully] region=<name>` / `// [SaidIgnore] slot=<name>` /
/// `// [SaidMerge] region=<name>`. Tolerates a UTF-8 BOM at the start of
/// the line — VS / SSDT often saves .cs files with BOM and `trim_start`
/// only strips ASCII whitespace.
fn parse_open(line: &str) -> Option<(ManagedMode, String)> {
    let trimmed = line.trim_start_matches('\u{feff}').trim_start();
    let rest = trimmed.strip_prefix("//")?.trim_start();
    // Match `[SaidXxx]` attribute-style tag.
    let rest = rest.strip_prefix('[')?;
    let (tag, rest) = rest.split_once(']')?;
    let mode = match tag.trim() {
        "SaidFully" => ManagedMode::Fully,
        "SaidIgnore" => ManagedMode::Ignore,
        "SaidMerge" => ManagedMode::Merge,
        _ => return None,
    };
    let rest = rest.trim_start();
    // Expect `region=<name>` or `slot=<name>`.
    let mut parts = rest.split_whitespace();
    let kv = parts.next()?;
    let name = kv
        .strip_prefix("region=")
        .or_else(|| kv.strip_prefix("slot="))?;
    Some((mode, name.to_string()))
}

fn is_end(line: &str) -> bool {
    let trimmed = line.trim_start_matches('\u{feff}').trim_start();
    let rest = match trimmed.strip_prefix("//") {
        Some(r) => r.trim_start(),
        None => return false,
    };
    let rest = match rest.strip_prefix('[') {
        Some(r) => r,
        None => return false,
    };
    let (tag, _) = match rest.split_once(']') {
        Some(t) => t,
        None => return false,
    };
    tag.trim() == "SaidEnd"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_header() {
        let text = "// [SaidFully] region=file-header\n//*** stuff ***/\n// LogNr: ...\n//*** stuff ***/\n// [SaidEnd]\n";
        let regions = parse_cs_regions(text).unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].mode, ManagedMode::Fully);
        assert_eq!(regions[0].name, "file-header");
    }

    #[test]
    fn parses_indented_ignore() {
        let text = "    // [SaidIgnore] slot=author-content\n    var x = 5;\n    // [SaidEnd]\n";
        let regions = parse_cs_regions(text).unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].mode, ManagedMode::Ignore);
        assert_eq!(regions[0].name, "author-content");
    }

    #[test]
    fn malformed_returns_error() {
        let text = "// [SaidFully] region=foo\nno close\n";
        assert!(parse_cs_regions(text).is_err());
    }

    #[test]
    fn ignores_non_marker_comments() {
        let text = "// This is just a comment\n// Another comment\n";
        let regions = parse_cs_regions(text).unwrap();
        assert_eq!(regions.len(), 0);
    }
}
