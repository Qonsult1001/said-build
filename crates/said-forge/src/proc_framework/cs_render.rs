//! C# renderer — Phase 2a of the proc-framework migration.
//!
//! Mirrors `render.rs` but for C# Query classes. Reads the cs shape
//! compose-list at `profiles/<X>/cs/_shapes/{command,query-by-id,
//! query-list}.toml`, emits a complete `.cs` file with `// [SaidFully]`
//! markers around the framework-owned regions and preserves any
//! `// [SaidIgnore]` author content from the deployed file.
//!
//! The shape template uses three `Open` region kinds the renderer emits
//! inline (same pattern as SQL's `create-procedure` / `end-procedure`):
//!
//!   - `usings-and-namespace`  the `using X;\nnamespace Y;` header
//!   - `class-declaration`     `[ExcludeFromCodeCoverage] public class XQuery {`
//!   - `query-method`          the static Query() method with the EXEC string
//!   - `end-class`             `}` to close the class
//!
//! The Fully regions (file-header.cs, query-class-properties.cs,
//! ef-mapping.cs) live as fragment files under `cs/_shared/` and are
//! emitted verbatim with `{{var}}` substitution — identical mechanism
//! to SQL.

use std::collections::BTreeMap;
use std::path::Path;

use super::cs_markers::parse_cs_regions;
use super::manifest::EndpointRow;
use super::markers::ManagedMode;
use super::shape::{load_fragment_cs, load_shape_cs, ShapeRegion};

/// Render one endpoint's Query class.
///
/// `framework_root` is the path to `dtcard/.forge/proc-framework/`.
/// `preserve_ignores_from` is the deployed `.cs` file path — if it
/// exists and contains `// [SaidIgnore]` regions, they're carried over.
pub fn render_endpoint_cs(
    framework_root: &Path,
    profile: &str,
    row: &EndpointRow,
    preserve_ignores_from: Option<&Path>,
) -> Result<String, String> {
    let shape = load_shape_cs(framework_root, profile, &row.shape)?;
    let mut out = Vec::<String>::new();

    let base_vars = build_base_vars_cs(row);

    // Carry over Ignore-region content from the deployed file.
    let existing_ignores: BTreeMap<String, String> = match preserve_ignores_from {
        Some(p) if p.exists() => {
            let text = std::fs::read_to_string(p)
                .map_err(|e| format!("read {}: {}", p.display(), e))?;
            extract_cs_ignore_regions(&text)?
        }
        _ => BTreeMap::new(),
    };

    for region in &shape.regions {
        emit_region_cs(
            framework_root,
            profile,
            row,
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

/// Standard substitutions every fragment may reference. C# variant —
/// adds `query_class_name`, `request_dto`, `route_params_cs`,
/// `route_params_exec`, `bundle_folder`, plus the header-block vars
/// reused from `render.rs::build_base_vars`.
fn build_base_vars_cs(row: &EndpointRow) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();

    // Header-block defaults (same shape as SQL renderer).
    vars.insert(
        "author".into(),
        row.author
            .clone()
            .unwrap_or_else(|| "Willie Olivier - AI (framework bundle)".into()),
    );
    vars.insert(
        "create_date".into(),
        row.create_date.clone().unwrap_or_else(|| "2026-05-12".into()),
    );
    let raw_desc = row.description.clone().unwrap_or_default();
    vars.insert("description".into(), raw_desc.trim().to_string());

    // Synthesised single-row change log when the bundle doesn't carry one.
    let change_log = if row.change_log_rows.is_empty() {
        let bundle = row.id.split('.').next().unwrap_or("");
        if bundle.is_empty() {
            "// BUNDLE     20260512  1.0.0  Willie Olivier - AI       Rendered from bundle".to_string()
        } else {
            format!(
                "// BUNDLE     20260512  1.0.0  Willie Olivier - AI       Rendered from bundle {}",
                bundle
            )
        }
    } else {
        row.change_log_rows
            .iter()
            .map(|r| {
                // SQL change-log lines start with `--`; the cs fragment expects
                // `//` style line comments. Rewrite the leading `--` to `//`
                // so a bundle that supplies SQL-style change_log_rows also
                // renders correctly into a .cs file. No-op if already `//`.
                if let Some(rest) = r.strip_prefix("-- ") {
                    format!("// {}", rest)
                } else if let Some(rest) = r.strip_prefix("--") {
                    format!("//{}", rest)
                } else {
                    r.clone()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    vars.insert("change_log_rows".into(), change_log);

    // C#-specific vars.
    vars.insert("query_class_name".into(), format!("{}Query", row.cs_class_stem()));
    vars.insert("class_stem".into(), row.cs_class_stem());
    vars.insert("bundle_folder".into(), row.cs_bundle_folder());
    vars.insert(
        "request_dto".into(),
        row.request_dto
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "object".into()),
    );
    vars.insert(
        "response_dto".into(),
        row.response_dto.clone().unwrap_or_default(),
    );
    vars.insert("schema".into(), row.schema.clone().unwrap_or_default());
    vars.insert("entity".into(), row.entity.clone());
    vars.insert("verb".into(), row.verb.clone());
    vars.insert("api_id".into(), row.api_id.clone());
    vars.insert("proc_name".into(), row.proc_name());

    // Route params:
    //   - `route_params_cs`   — appends `, Guid accountId` to method signature
    //   - `route_params_exec` — appends `,\n        {accountId}` to EXEC string
    //
    // Each RouteParam carries an SQL name (`@uAccountId`, UNIQUEIDENTIFIER).
    // Map to a C# camelCase identifier + Guid type for the signature, and
    // reuse the same identifier as the EXEC interpolation token.
    let mut sig = String::new();
    let mut exec = String::new();
    for p in &row.route_params {
        let camel = sql_param_to_camel(&p.name);
        let cs_ty = sql_type_to_cs(&p.ty);
        sig.push_str(&format!(", {} {}", cs_ty, camel));
        exec.push_str(&format!(",\n        {{{}}}", camel));
    }
    vars.insert("route_params_cs".into(), sig);
    vars.insert("route_params_exec".into(), exec);

    vars
}

/// Convert SQL parameter name to C# camelCase identifier.
///   `@uAccountId`  → `accountId`
///   `@uTransitionId` → `transitionId`
///   `@iPage`       → `page`
fn sql_param_to_camel(sql_name: &str) -> String {
    // Strip leading '@'.
    let s = sql_name.strip_prefix('@').unwrap_or(sql_name);
    // Strip Hungarian prefix (1–2 lowercase letters): `uAccountId` → `AccountId`,
    // `sName` → `Name`, `iPage` → `Page`. Be defensive — if no Hungarian
    // prefix, take the input as-is and just lowercase the first char.
    let stripped = if let Some(stripped) = strip_hungarian(s) {
        stripped
    } else {
        s.to_string()
    };
    // Lower-case the first letter to camelCase.
    let mut chars = stripped.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Strip a Hungarian prefix of 1–2 lowercase ASCII letters followed by
/// an uppercase letter. Returns the remainder (`uAccountId` → `AccountId`).
/// Returns None when the string doesn't look Hungarian-prefixed.
fn strip_hungarian(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    // Need at least: 1 lowercase + 1 uppercase = 2 chars.
    if bytes.len() < 2 {
        return None;
    }
    let mut i = 0;
    while i < bytes.len() && i < 2 && bytes[i].is_ascii_lowercase() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    if i < bytes.len() && bytes[i].is_ascii_uppercase() {
        Some(s[i..].to_string())
    } else {
        None
    }
}

/// Map SQL type to C# type for method signatures.
fn sql_type_to_cs(sql_ty: &str) -> &'static str {
    let upper = sql_ty.trim().to_uppercase();
    if upper.contains("UNIQUEIDENTIFIER") {
        "Guid"
    } else if upper.contains("INT") || upper.contains("BIGINT") {
        "int"
    } else if upper.contains("BIT") {
        "bool"
    } else if upper.contains("DATETIME") {
        "DateTime"
    } else if upper.contains("MONEY") || upper.contains("DECIMAL") {
        "decimal"
    } else {
        // NVARCHAR / VARCHAR / CHAR / etc.
        "string"
    }
}

fn emit_region_cs(
    framework_root: &Path,
    profile: &str,
    row: &EndpointRow,
    region: &ShapeRegion,
    base_vars: &BTreeMap<String, String>,
    existing_ignores: &BTreeMap<String, String>,
    out: &mut Vec<String>,
) -> Result<(), String> {
    match region.mode.as_str() {
        "Fully" => {
            let fragment_name = region
                .fragment
                .as_ref()
                .ok_or_else(|| "Fully region missing `fragment` field".to_string())?;
            let mut text = load_fragment_cs(framework_root, profile, fragment_name)?;
            let mut vars = base_vars.clone();
            for (k, v) in &region.vars {
                vars.insert(k.clone(), toml_to_string(v));
            }
            text = substitute(&text, &vars);
            while text.ends_with('\n') {
                text.pop();
            }
            out.push(text);
            out.push(String::new()); // blank line between regions
        }
        "Open" => match region.kind.as_deref() {
            Some("usings-and-namespace") => emit_usings_and_namespace(row, out),
            Some("class-declaration") => emit_class_declaration(row, base_vars, out),
            Some("query-method") => emit_query_method(row, base_vars, out),
            Some(other) => {
                return Err(format!(
                    "unknown Open kind '{}' in cs shape — supported: usings-and-namespace, class-declaration, query-method",
                    other
                ));
            }
            None => {
                if let Some(raw) = &region.raw {
                    out.push(raw.clone());
                }
            }
        },
        "Close" => match region.kind.as_deref() {
            Some("end-class") => out.push("}".into()),
            Some(other) => return Err(format!("unknown Close kind '{}'", other)),
            None => {}
        },
        "Ignore" => {
            let slot = region
                .slot
                .as_ref()
                .ok_or_else(|| "Ignore region missing `slot` field".to_string())?;
            let indent = "    "; // inside-class default indent
            if let Some(preserved) = existing_ignores.get(slot) {
                out.push(preserved.clone());
            } else {
                let comment = region.comment.clone().unwrap_or_else(|| slot.clone());
                out.push(format!("{}// [SaidIgnore] slot={}", indent, slot));
                out.push(format!("{}// TODO author: {}", indent, comment.to_lowercase()));
                out.push(format!("{}// [SaidEnd]", indent));
            }
            out.push(String::new());
        }
        other => return Err(format!("unknown region mode '{}'", other)),
    }
    Ok(())
}

fn emit_usings_and_namespace(row: &EndpointRow, out: &mut Vec<String>) {
    // Standard usings every Query class needs.
    out.push("using System.Diagnostics.CodeAnalysis;".into());
    out.push("using DirectTransact.TxnGlobal.API.Models.Common;".into());
    // Request DTO usings — match the deployed convention of importing
    // from `Models.Request.{Bundle}` and (for query shapes returning data)
    // `Models.Response.{Bundle}`.
    let bundle = row.cs_bundle_folder();
    if let Some(_dto) = row.request_dto.as_ref().filter(|s| !s.is_empty()) {
        if !bundle.is_empty() {
            out.push(format!(
                "using DirectTransact.TxnGlobal.API.Models.Request.{};",
                bundle
            ));
        }
    }
    out.push("using Microsoft.EntityFrameworkCore;".into());
    out.push(String::new());
    // Namespace. Nested folder becomes a nested namespace segment.
    if bundle.is_empty() {
        out.push("namespace DirectTransact.TxnGlobal.API.Data.Repositories.SqlQueries;".into());
    } else {
        out.push(format!(
            "namespace DirectTransact.TxnGlobal.API.Data.Repositories.SqlQueries.{};",
            bundle
        ));
    }
    out.push(String::new());
}

fn emit_class_declaration(row: &EndpointRow, base_vars: &BTreeMap<String, String>, out: &mut Vec<String>) {
    let desc = base_vars.get("description").cloned().unwrap_or_default();
    out.push("/// <summary>".into());
    if desc.is_empty() {
        out.push(format!("/// SQL query model for {}.", row.id));
    } else {
        for line in desc.lines() {
            out.push(format!("/// {}", line));
        }
    }
    out.push("/// </summary>".into());
    out.push("[ExcludeFromCodeCoverage]".into());
    out.push(format!("public class {}Query", row.cs_class_stem()));
    out.push("{".into());
}

fn emit_query_method(row: &EndpointRow, base_vars: &BTreeMap<String, String>, out: &mut Vec<String>) {
    // Method XML doc.
    out.push("    /// <summary>".into());
    out.push(format!("    /// Builds the SQL query string for {}.", row.verb.to_lowercase()));
    out.push("    /// </summary>".into());
    out.push("    /// <param name=\"requestContext\">The request context containing payload and metadata.</param>".into());
    for p in &row.route_params {
        let camel = sql_param_to_camel(&p.name);
        out.push(format!(
            "    /// <param name=\"{}\">The {} from the route.</param>",
            camel,
            camel
        ));
    }
    out.push("    /// <returns>A formattable string representing the stored procedure call.</returns>".into());

    // Method signature.
    let request_dto = base_vars.get("request_dto").cloned().unwrap_or_else(|| "object".into());
    let route_sig = base_vars.get("route_params_cs").cloned().unwrap_or_default();
    out.push(format!(
        "    public static FormattableString Query(RequestContext<{}> requestContext{})",
        request_dto, route_sig
    ));
    out.push("    {".into());

    // EXEC string. Two flavours based on shape: command/query-by-id use
    // the JsonPayload+RequestId+CorrelationId+RequestHeaders+ApiId
    // positional order; query-list uses paging-then-ApiId. Mirror the
    // SQL shape's parameter signature.
    let schema = base_vars.get("schema").cloned().unwrap_or_default();
    let proc_name = base_vars.get("proc_name").cloned().unwrap_or_default();
    let route_exec = base_vars.get("route_params_exec").cloned().unwrap_or_default();

    let exec_body = match row.shape.as_str() {
        "command" => {
            format!(
                "$@\"EXEC [{}].[{}]\n        {{requestContext.JsonPayload}},\n        {{requestContext.RequestId}},\n        {{requestContext.CorrelationId}},\n        {{requestContext.RequestHeaders}},\n        {{requestContext.ApiId}}{}\"",
                schema, proc_name, route_exec
            )
        }
        "query-by-id" => {
            format!(
                "$@\"EXEC [{}].[{}]\n        {{requestContext.RequestId}},\n        {{requestContext.CorrelationId}},\n        {{requestContext.RequestHeaders}},\n        {{requestContext.ApiId}}{}\"",
                schema, proc_name, route_exec
            )
        }
        "query-list" => {
            // Paging vars come from `int page, int limit, string sortingParameters`
            // added to the method signature by an `route_params_cs`-like
            // mechanism. For Phase 2a we encode the canonical query-list
            // signature directly.
            format!(
                "$@\"EXEC [{}].[{}]\n        {{requestContext.RequestId}},\n        {{requestContext.CorrelationId}},\n        {{requestContext.RequestHeaders}},\n        {{page}},\n        {{limit}},\n        {{requestContext.ApiId}},\n        {{sortingParameters}}{}\"",
                schema, proc_name, route_exec
            )
        }
        other => {
            // Unknown shape — fall back to command flavour.
            format!(
                "$@\"EXEC [{}].[{}] /* unknown shape '{}' — falling back to command flavour */ {{requestContext.JsonPayload}}, {{requestContext.RequestId}}, {{requestContext.CorrelationId}}, {{requestContext.RequestHeaders}}, {{requestContext.ApiId}}{}\"",
                schema, proc_name, other, route_exec
            )
        }
    };
    out.push(format!("        return {};", exec_body));
    out.push("    }".into());
}

/// Extract `// [SaidIgnore] slot=<name> ... // [SaidEnd]` regions from
/// existing C# content. Reuses the shared cs_markers parser.
fn extract_cs_ignore_regions(text: &str) -> Result<BTreeMap<String, String>, String> {
    let regions = parse_cs_regions(text)?;
    let mut map = BTreeMap::new();
    for r in regions {
        if r.mode == ManagedMode::Ignore {
            let indent = detect_indent_cs(&r.body);
            let pad = " ".repeat(indent);
            let block = format!(
                "{}// [SaidIgnore] slot={}\n{}\n{}// [SaidEnd]",
                pad, r.name, r.body, pad
            );
            map.insert(r.name.clone(), block);
        }
    }
    Ok(map)
}

fn detect_indent_cs(body: &str) -> usize {
    for line in body.lines() {
        let trimmed = line.trim_start();
        if !trimmed.is_empty() {
            return line.len() - trimmed.len();
        }
    }
    0
}

// --- shared helpers (same as render.rs::substitute) ------------------------

fn substitute(text: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end_rel) = find_subseq(&bytes[i + 2..], b"}}") {
                let inner = &text[i + 2..i + 2 + end_rel];
                let key = inner.trim();
                if is_identifier(key) {
                    match vars.get(key) {
                        Some(v) => out.push_str(v),
                        None => out.push_str(&text[i..i + 2 + end_rel + 2]),
                    }
                    i = i + 2 + end_rel + 2;
                    continue;
                }
            }
        }
        // Push the next character as UTF-8-safe — use the char at this
        // byte offset if it's a char-boundary.
        if text.is_char_boundary(i) {
            let ch_start = i;
            let ch_end = (i + 1..=text.len())
                .find(|&j| text.is_char_boundary(j))
                .unwrap_or(text.len());
            out.push_str(&text[ch_start..ch_end]);
            i = ch_end;
        } else {
            i += 1;
        }
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

// --- tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hungarian_strip() {
        assert_eq!(strip_hungarian("uAccountId"), Some("AccountId".into()));
        assert_eq!(strip_hungarian("sName"), Some("Name".into()));
        assert_eq!(strip_hungarian("iPage"), Some("Page".into()));
        assert_eq!(strip_hungarian("AccountId"), None);
        assert_eq!(strip_hungarian(""), None);
    }

    #[test]
    fn sql_param_camel() {
        assert_eq!(sql_param_to_camel("@uAccountId"), "accountId");
        assert_eq!(sql_param_to_camel("@uTransitionId"), "transitionId");
        assert_eq!(sql_param_to_camel("@iPage"), "page");
        assert_eq!(sql_param_to_camel("@sSortingParameters"), "sortingParameters");
    }

    #[test]
    fn sql_type_map() {
        assert_eq!(sql_type_to_cs("UNIQUEIDENTIFIER"), "Guid");
        assert_eq!(sql_type_to_cs("INT"), "int");
        assert_eq!(sql_type_to_cs("BIGINT"), "int");
        assert_eq!(sql_type_to_cs("NVARCHAR(200)"), "string");
        assert_eq!(sql_type_to_cs("BIT"), "bool");
    }
}
