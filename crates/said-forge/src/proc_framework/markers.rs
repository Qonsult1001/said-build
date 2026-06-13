//! `@said-managed` marker grammar — Fully | Ignore | Merge.
//!
//! Marker syntax (SQL):
//!   `-- @said-managed: Fully  region=<name>`
//!   `-- @said-managed: Ignore slot=<name>`
//!   `-- @said-managed: Merge  region=<name>`
//!   `-- @said-managed: end`
//!
//! Indentation is preserved verbatim — the marker is matched on a per-line
//! basis after stripping leading whitespace. The closing `end` marker
//! belongs to whichever Fully/Ignore/Merge block opened most recently
//! (markers don't nest in the current grammar).

use serde::{Deserialize, Serialize};

/// Three modes — same enum the Python tool emits + audits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ManagedMode {
    /// Framework-owned region. Render overwrites; audit checks equality.
    Fully,
    /// Author-owned region. Render preserves verbatim; audit only verifies
    /// the marker pair exists.
    Ignore,
    /// Mixed-ownership region. Framework adds; preserves author additions.
    /// Rare today — not yet exercised by any shape.
    Merge,
}

impl ManagedMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ManagedMode::Fully => "Fully",
            ManagedMode::Ignore => "Ignore",
            ManagedMode::Merge => "Merge",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Fully" => Some(ManagedMode::Fully),
            "Ignore" => Some(ManagedMode::Ignore),
            "Merge" => Some(ManagedMode::Merge),
            _ => None,
        }
    }
}

/// One marker-wrapped region in a deployed file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Region {
    pub mode: ManagedMode,
    /// `region=<name>` for Fully/Merge, `slot=<name>` for Ignore.
    pub name: String,
    /// Body text between the open and close marker lines (exclusive).
    pub body: String,
    /// 1-based line number of the open marker line.
    pub line_start: usize,
    /// 1-based line number of the close marker line.
    pub line_end: usize,
}

/// Parse every `@said-managed` region in `text`. Returns regions in file
/// order. Malformed markers (open without close) cause an error rather than
/// silent drop — better to fail loudly during audit than miss drift.
pub fn parse_regions(text: &str) -> Result<Vec<Region>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut regions = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some((mode, name)) = parse_open_marker(lines[i]) {
            // Find the matching `end` line.
            let mut j = i + 1;
            let mut found_end = false;
            while j < lines.len() {
                if is_end_marker(lines[j]) {
                    found_end = true;
                    break;
                }
                j += 1;
            }
            if !found_end {
                return Err(format!(
                    "malformed marker at line {}: '{}' has no closing -- @said-managed: end",
                    i + 1,
                    lines[i].trim()
                ));
            }
            // Body = lines between open (exclusive) and end (exclusive).
            let body = if j > i + 1 {
                lines[i + 1..j].join("\n")
            } else {
                String::new()
            };
            regions.push(Region {
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

/// Match a line against the open-marker grammar. Returns (mode, name) on
/// success.
///
/// Examples that match:
///   `-- @said-managed: Fully  region=header`
///   `        -- @said-managed: Ignore  slot=5_vars`
///   `    -- @said-managed: Merge region=foo`
fn parse_open_marker(line: &str) -> Option<(ManagedMode, String)> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("--")?.trim_start();
    let rest = rest.strip_prefix("@said-managed:")?.trim_start();
    // Skip case where rest starts with "end" — that's a close marker.
    if rest.starts_with("end") {
        return None;
    }
    // Split on whitespace: <Mode> <region|slot>=<name>
    let mut parts = rest.split_whitespace();
    let mode_token = parts.next()?;
    let mode = ManagedMode::from_str(mode_token)?;
    let kv = parts.next()?;
    // Accept `region=` for Fully/Merge, `slot=` for Ignore (but tolerate
    // either in the parser — auditor enforces the per-mode convention).
    let name = kv.strip_prefix("region=").or_else(|| kv.strip_prefix("slot="))?;
    Some((mode, name.to_string()))
}

/// `-- @said-managed: end` — accept any leading whitespace.
fn is_end_marker(line: &str) -> bool {
    let trimmed = line.trim_start();
    let rest = match trimmed.strip_prefix("--") {
        Some(r) => r.trim_start(),
        None => return false,
    };
    let rest = match rest.strip_prefix("@said-managed:") {
        Some(r) => r.trim_start(),
        None => return false,
    };
    rest.trim_end() == "end"
}

/// Extract Ignore-region bodies indexed by slot name, for the renderer to
/// preserve author content across re-renders.
pub fn extract_ignore_regions(text: &str) -> Result<std::collections::BTreeMap<String, String>, String> {
    let regions = parse_regions(text)?;
    let mut map = std::collections::BTreeMap::new();
    for r in regions {
        if r.mode == ManagedMode::Ignore {
            // Preserve the FULL block including its marker lines so the
            // renderer can splice it back verbatim.
            let block = format!(
                "{}\n{}\n{}",
                spaces(detect_indent_len(&r.body)) + "-- @said-managed: Ignore  slot=" + &r.name,
                r.body,
                spaces(detect_indent_len(&r.body)) + "-- @said-managed: end"
            );
            map.insert(r.name, block);
        }
    }
    Ok(map)
}

fn detect_indent_len(body: &str) -> usize {
    for line in body.lines() {
        let trimmed = line.trim_start();
        if !trimmed.is_empty() {
            return line.len() - trimmed.len();
        }
    }
    0
}

fn spaces(n: usize) -> String {
    " ".repeat(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_fully_region() {
        let text = "-- @said-managed: Fully  region=header\nALPHA\nBETA\n-- @said-managed: end\n";
        let regions = parse_regions(text).unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].mode, ManagedMode::Fully);
        assert_eq!(regions[0].name, "header");
        assert_eq!(regions[0].body, "ALPHA\nBETA");
    }

    #[test]
    fn parses_indented_ignore_slot() {
        let text = "        -- @said-managed: Ignore  slot=5_vars\n        DECLARE @x INT;\n        -- @said-managed: end\n";
        let regions = parse_regions(text).unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].mode, ManagedMode::Ignore);
        assert_eq!(regions[0].name, "5_vars");
    }

    #[test]
    fn malformed_returns_error() {
        let text = "-- @said-managed: Fully  region=header\nno close marker\n";
        assert!(parse_regions(text).is_err());
    }
}
