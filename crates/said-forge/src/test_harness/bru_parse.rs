//! Tiny `.bru` file parser. The Bruno format is line-oriented blocks
//! delimited by `name { ... }`. We extract: meta.name, http verb +
//! url, headers, body:json, vars:pre-request.
//!
//! No external dep — Bruno files are small and the format is
//! predictable. We don't support every Bruno feature; just enough to
//! drive the synthetic HTTP server.

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct BruRequest {
    /// File-name stem (e.g. `CreateBinSponsor`).
    pub name: String,
    /// Folder name above the file (e.g. `BinSponsor`).
    pub entity_folder: String,
    /// HTTP method, uppercase.
    pub method: String,
    /// URL with `{{var}}` placeholders intact.
    pub url: String,
    pub headers: BTreeMap<String, String>,
    /// Body JSON if `body:json { ... }` block is present.
    pub body_json: Option<String>,
    /// Pre-request variables (e.g. fixture id values).
    pub pre_request_vars: BTreeMap<String, String>,
    /// Sequence number from `meta.seq` (controls within-folder ordering).
    pub seq: u32,
}

impl BruRequest {
    /// `true` when this fixture is a negative-test case (filename starts
    /// with `neg-`). Negative fixtures invert the harness's verdict —
    /// a 4xx with the expected PrcCode is a *pass*, a 2xx is a *fail*
    /// (the API was supposed to reject the request).
    pub fn is_negative(&self) -> bool {
        self.name.starts_with("neg-") || self.name.starts_with("Neg-")
    }

    /// Expected HTTP status for a negative fixture, parsed from
    /// `vars:pre-request { expected_status: 400 }`. Returns `None` when
    /// not declared — the harness then accepts any 4xx as the rejection
    /// signal.
    pub fn expected_status(&self) -> Option<u16> {
        self.pre_request_vars
            .get("expected_status")
            .and_then(|s| s.trim().parse().ok())
    }

    /// Expected error code for a negative fixture. Accepts either:
    ///   - `expected_prc_code: 1000`         (legacy numeric prc)
    ///   - `expected_error_code: CARD-Q-100` (new erc string code)
    /// When both are declared, `expected_error_code` wins. Returns
    /// `None` when neither is set — the harness then accepts any
    /// code in the response envelope as long as the status matches.
    pub fn expected_prc_code(&self) -> Option<String> {
        if let Some(s) = self
            .pre_request_vars
            .get("expected_error_code")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            return Some(s);
        }
        self.pre_request_vars
            .get("expected_prc_code")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

/// Walk a Bruno collection root (e.g. `1. Local`) and parse every
/// `*.bru` request file. `folder.bru` files are skipped.
pub fn load_collection(root: &Path) -> Result<Vec<BruRequest>, String> {
    if !root.exists() {
        return Err(format!("collection root missing: {}", root.display()));
    }
    let mut out = Vec::new();
    walk(root, &mut out)?;
    out.sort_by(|a, b| {
        a.entity_folder.cmp(&b.entity_folder)
            .then_with(|| a.seq.cmp(&b.seq))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<BruRequest>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("read_dir {}: {}", dir.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out)?;
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !name.ends_with(".bru") || name == "folder.bru" {
            continue;
        }
        let entity_folder = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("read {}: {}", path.display(), e))?;
        if let Some(req) = parse_bru(&text, name, &entity_folder) {
            out.push(req);
        }
    }
    Ok(())
}

fn parse_bru(text: &str, filename: &str, entity_folder: &str) -> Option<BruRequest> {
    let mut blocks: BTreeMap<String, String> = BTreeMap::new();
    let mut current_name: Option<String> = None;
    let mut current_body = String::new();
    let mut depth: i32 = 0;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if depth == 0 {
            // Look for `name {`.
            if let Some(brace_pos) = trimmed.find('{') {
                let head = trimmed[..brace_pos].trim().to_string();
                if !head.is_empty() {
                    current_name = Some(head);
                    depth = 1;
                    // Track any payload after `{` on the same line (rare in Bruno).
                    let after = &trimmed[brace_pos + 1..];
                    if !after.is_empty() {
                        current_body.push_str(after);
                        current_body.push('\n');
                    }
                    continue;
                }
            }
        } else {
            // Inside a block; track nested braces.
            for c in trimmed.chars() {
                if c == '{' { depth += 1; }
                else if c == '}' { depth -= 1; }
            }
            if depth == 0 {
                // Closing brace reached. The line being processed
                // contains the closing `}` and possibly trailing
                // chars on its own — we do NOT push it to body, so
                // body is already clean.
                if let Some(name) = current_name.take() {
                    blocks.insert(name, current_body.trim_end().to_string());
                }
                current_body.clear();
                continue;
            } else {
                current_body.push_str(line);
                current_body.push('\n');
            }
        }
    }

    // meta — `name: ...`, `seq: ...`
    let meta = blocks.get("meta").map(String::as_str).unwrap_or("");
    let req_name = parse_kv_lines(meta).get("name").cloned()
        .unwrap_or_else(|| filename.trim_end_matches(".bru").to_string());
    let seq: u32 = parse_kv_lines(meta).get("seq")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // method block — exactly one of `get`, `post`, `put`, `patch`, `delete`.
    let mut method = String::new();
    let mut url = String::new();
    for verb in ["get", "post", "put", "patch", "delete"] {
        if let Some(body) = blocks.get(verb) {
            method = verb.to_uppercase();
            // First line is `url: <url>`.
            for line in body.lines() {
                let t = line.trim();
                if let Some(rest) = t.strip_prefix("url:") {
                    url = rest.trim().to_string();
                    break;
                }
            }
            break;
        }
    }
    if method.is_empty() {
        return None;
    }

    // headers
    let headers_block = blocks.get("headers").map(String::as_str).unwrap_or("");
    let headers = parse_kv_lines(headers_block);

    // body:json — block name is literally "body:json"
    let body_json = blocks.get("body:json").map(|s| s.trim().to_string());

    // vars:pre-request
    let pre_block = blocks.get("vars:pre-request").map(String::as_str).unwrap_or("");
    let pre_request_vars = parse_kv_lines(pre_block);

    Some(BruRequest {
        name: req_name,
        entity_folder: entity_folder.to_string(),
        method,
        url,
        headers,
        body_json,
        pre_request_vars,
        seq,
    })
}

fn parse_kv_lines(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(idx) = t.find(':') {
            let k = t[..idx].trim().to_string();
            let v = t[idx + 1..].trim().to_string();
            if !k.is_empty() {
                out.insert(k, v);
            }
        }
    }
    out
}
