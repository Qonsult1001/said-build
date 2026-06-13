//! Renderer — turns an `EndpointRow` + its shape into a complete proc file.
//!
//! Equivalent to the Python `core/render.py::render_endpoint`. Preserves
//! Ignore-region author content when an existing deployed file is provided.

use std::collections::BTreeMap;
use std::path::Path;

use super::manifest::EndpointRow;
use super::markers::extract_ignore_regions;
use super::shape::{load_fragment, load_shape, ShapeRegion};

/// Render one endpoint to SQL. `framework_root` is the path to
/// `dtcard/.forge/proc-framework/`. `preserve_ignores_from` is the path to
/// the deployed file if it exists — the renderer extracts its Ignore regions
/// verbatim and splices them into the output.
pub fn render_endpoint(
    framework_root: &Path,
    profile: &str,
    row: &EndpointRow,
    preserve_ignores_from: Option<&Path>,
) -> Result<String, String> {
    let shape = load_shape(framework_root, profile, &row.shape)?;
    let mut out = Vec::<String>::new();

    let base_vars = build_base_vars(row);

    // Pre-load existing Ignore regions if we have a deployed file.
    let existing_ignores: BTreeMap<String, String> = match preserve_ignores_from {
        Some(p) if p.exists() => {
            let text = std::fs::read_to_string(p)
                .map_err(|e| format!("read {}: {}", p.display(), e))?;
            extract_ignore_regions(&text)?
        }
        _ => BTreeMap::new(),
    };

    for region in &shape.regions {
        // Skip conditional regions whose flag is false.
        if let Some(flag) = &region.if_flag {
            if !flag_true(row, flag) {
                continue;
            }
        }
        emit_region(
            framework_root,
            profile,
            row,
            &shape.parameter_signature,
            region,
            &base_vars,
            &existing_ignores,
            &mut out,
        )?;
    }

    let mut text = out.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

/// Build the standard substitution map every fragment may reference.
fn build_base_vars(row: &EndpointRow) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();

    // Match the Python tool's bundle-default behaviour: when a bundle row
    // doesn't specify author/create_date/change_log_rows explicitly, fall
    // back to bundle-attributed defaults (not generic "Willie Olivier - AI"
    // and empty fields).
    vars.insert(
        "author".into(),
        row.author
            .clone()
            .unwrap_or_else(|| "Willie Olivier - AI (framework bundle)".into()),
    );
    vars.insert(
        "create_date".into(),
        row.create_date.clone().unwrap_or_else(|| "2026-05-11".into()),
    );

    // Description: multiline descriptions get continuation-prefixed with
    // "--                " so they remain valid SQL comments.
    let raw_desc = row.description.clone().unwrap_or_default();
    let raw_desc = raw_desc.trim();
    let desc_lines: Vec<&str> = raw_desc.lines().collect();
    let description = if desc_lines.len() > 1 {
        let prefix = "--                ";
        let head = desc_lines[0].to_string();
        let tail: Vec<String> = desc_lines[1..].iter().map(|l| format!("{}{}", prefix, l)).collect();
        format!("{}\n{}", head, tail.join("\n"))
    } else {
        raw_desc.to_string()
    };
    vars.insert("description".into(), description);

    vars.insert("method".into(), row.method.clone().unwrap_or_default());
    vars.insert("path".into(), row.path.clone());
    // Same bundle-default rule for the change log: if the row didn't carry
    // its own, synthesise a single "Rendered from bundle <X>" entry. The
    // bundle name is inferred from the row id prefix (id format is
    // "<Bundle>.<Verb>" — e.g. "Account.CreateAccount"). Falls back to a
    // generic message if the id doesn't contain a dot.
    let change_log = if row.change_log_rows.is_empty() {
        let bundle_name = row.id.split('.').next().unwrap_or("");
        if bundle_name.is_empty() {
            "-- BUNDLE     20260511  1.0.0  Willie Olivier - AI       Rendered from bundle".to_string()
        } else {
            format!(
                "-- BUNDLE     20260511  1.0.0  Willie Olivier - AI       Rendered from bundle {}",
                bundle_name
            )
        }
    } else {
        row.change_log_rows.join("\n")
    };
    vars.insert("change_log_rows".into(), change_log);
    vars.insert("schema".into(), row.schema.clone().unwrap_or_default());
    vars.insert("entity".into(), row.entity.clone());
    vars.insert(
        "entity_plural".into(),
        row.entity_plural
            .clone()
            .unwrap_or_else(|| format!("{}s", row.entity)),
    );
    vars.insert("verb".into(), row.verb.clone());
    vars.insert("api_id".into(), row.api_id.clone());
    vars.insert("primary_table".into(), row.primary_table.clone());
    vars.insert("action_type".into(), row.action_type.clone());

    // Route param signature: ",\n@uX UNIQUEIDENTIFIER,\n@uY UNIQUEIDENTIFIER".
    // No leading indentation per line — `emit_create_procedure` prepends a
    // 4-space indent to every non-empty line of the rendered signature.
    let mut sig_extras = String::new();
    for p in &row.route_params {
        sig_extras.push_str(&format!(",\n{:<23} {}", p.name, p.ty));
    }
    // param_extras: non-canonical procedure parameters that exist in the
    // source proc but aren't in the canonical shape's parameter_signature.
    // Rendered after route_params, so the position-sensitive route params
    // stay where the harness's URL substitution expects them.
    for p in &row.param_extras {
        let mut line = format!(",\n{:<23} {}", p.name, p.ty);
        if let Some(d) = &p.default {
            line.push_str(&format!(" = {}", d));
        }
        sig_extras.push_str(&line);
    }
    vars.insert("route_params_sig".into(), sig_extras);

    vars
}

fn flag_true(row: &EndpointRow, flag: &str) -> bool {
    match flag {
        "has_validation" => row.has_validation,
        "has_child_tables" => row.has_child_tables,
        _ => false,
    }
}

fn emit_region(
    framework_root: &Path,
    profile: &str,
    row: &EndpointRow,
    parameter_signature: &str,
    region: &ShapeRegion,
    base_vars: &BTreeMap<String, String>,
    existing_ignores: &BTreeMap<String, String>,
    out: &mut Vec<String>,
) -> Result<(), String> {
    match region.mode.as_str() {
        "Open" => {
            if region.kind.as_deref() == Some("create-procedure") {
                emit_create_procedure(row, parameter_signature, base_vars, out);
            } else if let Some(raw) = &region.raw {
                out.push(raw.clone());
            }
        }
        "Close" => {
            if region.kind.as_deref() == Some("end-procedure") {
                out.push("END;".into());
                out.push("GO".into());
            }
        }
        "Fully" => {
            let fragment_name = region
                .fragment
                .as_ref()
                .ok_or_else(|| "Fully region missing `fragment` field".to_string())?;
            let mut text = load_fragment(framework_root, profile, fragment_name)?;
            // Merge per-region vars on top of base vars.
            let mut vars = base_vars.clone();
            for (k, v) in &region.vars {
                vars.insert(k.clone(), toml_to_string(v));
            }
            text = substitute(&text, &vars);
            // Strip trailing newlines; we add our own separator.
            while text.ends_with('\n') {
                text.pop();
            }
            out.push(text);
            out.push(String::new()); // blank line between regions
        }
        "Ignore" => {
            let slot = region
                .slot
                .as_ref()
                .ok_or_else(|| "Ignore region missing `slot` field".to_string())?;
            let indent = "        "; // in-TRY indent (8 spaces)
            if let Some(preserved) = existing_ignores.get(slot) {
                out.push(preserved.clone());
            } else {
                let comment = region.comment.clone().unwrap_or_else(|| slot.clone());
                out.push(format!("{}-- @said-managed: Ignore  slot={}", indent, slot));
                out.push(format!("{}-- TODO author: {}", indent, comment.to_lowercase()));
                out.push(format!("{}-- @said-managed: end", indent));
            }
            out.push(String::new());
        }
        other => {
            return Err(format!("unknown region mode '{}'", other));
        }
    }
    Ok(())
}

fn emit_create_procedure(
    row: &EndpointRow,
    parameter_signature: &str,
    base_vars: &BTreeMap<String, String>,
    out: &mut Vec<String>,
) {
    let schema = row.schema.as_deref().unwrap_or("");
    // SET QUOTED_IDENTIFIER ON must be set on the SESSION before CREATE
    // PROCEDURE — SQL Server captures the value at create time and bakes
    // it into the proc's metadata. Setting it inside the proc body is a
    // runtime no-op for purposes of INSERTs against indexed/filtered
    // tables (which check the create-time setting, not the run-time one).
    //
    // Several framework-managed tables hit Msg 1934 without this: e.g.
    // cardholder.add_Address_Details has filtered indexes
    // IX_add_Address_Shipping / IX_add_Address_Billing that demand
    // QUOTED_IDENTIFIER ON. The apply_sql_script harness splits on `GO`
    // markers, so the SET runs as its own batch ahead of CREATE.
    out.push("SET QUOTED_IDENTIFIER ON;".into());
    out.push("GO".into());
    out.push(String::new());
    out.push(format!(
        "CREATE PROCEDURE [{}].[{}]",
        schema,
        row.proc_name()
    ));
    // parameter_signature has its own internal indentation; we add 4 spaces
    // to each non-empty line to match the framework's output format.
    let resolved = substitute(parameter_signature, base_vars);
    let trimmed = resolved.trim_end();
    for line in trimmed.lines() {
        if line.trim().is_empty() {
            out.push(String::new());
        } else {
            out.push(format!("    {}", line));
        }
    }
    out.push("AS".into());
    out.push("BEGIN".into());
    out.push("    SET NOCOUNT ON;".into());
    out.push(String::new());
}

/// Replace `{{var}}` placeholders. Unknown vars are left intact so missing
/// keys are visible in the rendered output (fail loud, not silent-empty).
/// No regex dep — plain scan for `{{ ... }}` pairs.
fn substitute(text: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Find the closing `}}`.
            if let Some(end_rel) = find_subseq(&bytes[i + 2..], b"}}") {
                let inner = &text[i + 2..i + 2 + end_rel];
                let key = inner.trim();
                if is_identifier(key) {
                    match vars.get(key) {
                        Some(v) => out.push_str(v),
                        None => {
                            // Unknown var — emit verbatim so it's visible.
                            out.push_str(&text[i..i + 2 + end_rel + 2]);
                        }
                    }
                    i = i + 2 + end_rel + 2;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn find_subseq(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    for i in 0..=haystack.len() - needle.len() {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

fn is_identifier(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn toml_to_string(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        other => other.to_string(),
    }
}
