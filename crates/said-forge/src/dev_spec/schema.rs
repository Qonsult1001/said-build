//! Dev Spec request/response schema extraction.
//!
//! Reads the canonical `### Request Body` / per-status response `Schema:`
//! YAML blocks from a Dev Spec markdown file and returns a typed
//! `BodySchema`. This is the source of truth for Request DTO and Response
//! DTO generation under the proc framework — per ALIGNMENT Section 0:
//! "Dev Spec md > openapi-standard.toml rules > everything else".
//!
//! Why a hand-rolled YAML walker, not `serde_yaml`: the schema uses a
//! `Properties:` list of one-key maps (`- id:`, `- name (required):`)
//! where the key carries inline annotations (`(required)`). `serde_yaml`
//! would faithfully parse it as nested maps but recovering the annotation
//! and order is awkward. A scanner over the `Schema: ```yaml ... ``` `
//! fence is simpler and keeps order, which matters for emitted DTO
//! property order matching the deployed convention.

use std::collections::BTreeMap;
use std::path::Path;

/// One field in a request or response body. Order is preserved by the
/// caller's `Vec<BodyField>`.
#[derive(Debug, Clone, Default)]
pub struct BodyField {
    /// JSON property name as authored in the Dev Spec (camelCase).
    pub name: String,
    /// True when the YAML key is suffixed with `(required)`.
    pub required: bool,
    /// One of: `string`, `integer`, `number`, `boolean`, `array`,
    /// `object`. Free-form for forward-compat.
    pub yaml_type: String,
    /// `Format:` line — `uuid`, `date-time`, `email`, etc. Empty when
    /// absent.
    pub format: String,
    /// `Description:` line — used as the C# XML `<summary>` body. Empty
    /// when absent.
    pub description: String,
    /// `Example:` line — used as the C# `<example>` body. Empty when
    /// absent.
    pub example: String,
    /// `Enum: A, B, C` line — comma-split, trimmed, in document order.
    pub enums: Vec<String>,
    /// For `Type: array`, the inner `Items:` block's `Type:`. Empty for
    /// non-array fields.
    pub items_type: String,
    /// For `Type: array`, the inner `Items:` block's `Format:` (e.g.
    /// `uuid`). Empty when absent.
    pub items_format: String,
    /// `Max Length:` line — preserved as-string for forward-compat.
    pub max_length: String,
}

#[derive(Debug, Clone, Default)]
pub struct BodySchema {
    pub fields: Vec<BodyField>,
    /// `Required: a, b, c` line — top-level required list. Some specs
    /// repeat the requireds here in addition to the `(required)`
    /// suffix on individual fields.
    pub required_top_level: Vec<String>,
}

/// Parse `### Request Body` followed by a `Schema: ` ```yaml ... ``` `
/// fence. Returns `None` when no section yields a non-empty schema.
///
/// Some Dev Spec files contain two `### Request Body` sections — first
/// a JSON example, then the real `**Schema**:` block. We walk every
/// `### Request Body` and return the first one whose yaml fence parses
/// to a non-empty `Properties:` list. JSON examples deserialise to an
/// empty `BodySchema` and get skipped.
pub fn parse_request_body_schema(markdown: &str) -> Option<BodySchema> {
    for body_start in find_sections(markdown, "### Request Body") {
        let slice = &markdown[body_start..];
        // Stop at the next top-level heading so we don't drift into a
        // sibling section.
        let bounded = bound_to_next_top_heading(slice);
        if let Some(yaml) = extract_first_yaml_fence(bounded) {
            let parsed = parse_schema_yaml(&yaml);
            if !parsed.fields.is_empty() {
                return Some(parsed);
            }
        }
    }
    None
}

/// Limit a slice to the content before the next `### ` or `## `
/// heading. Used so the parser doesn't read schema content from the
/// response-status section while looking at Request Body.
fn bound_to_next_top_heading(slice: &str) -> &str {
    // Skip the first line (which is the heading we just matched).
    let after_first = match slice.find('\n') {
        Some(i) => i + 1,
        None => return slice,
    };
    let body = &slice[after_first..];
    let mut earliest = body.len();
    for needle in ["\n### ", "\n## ", "\n# "] {
        if let Some(idx) = body.find(needle) {
            if idx < earliest {
                earliest = idx;
            }
        }
    }
    &slice[..after_first + earliest]
}

fn find_sections(markdown: &str, heading: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut search_from = 0usize;
    while search_from < markdown.len() {
        let rest = &markdown[search_from..];
        let idx = match rest.find(heading) {
            Some(i) => i,
            None => break,
        };
        let abs = search_from + idx;
        let is_line_start = abs == 0 || markdown.as_bytes()[abs - 1] == b'\n';
        if is_line_start {
            out.push(abs);
        }
        search_from = abs + heading.len();
    }
    out
}

/// Parse the response body schema for a given status code (e.g. "201",
/// "200"). Looks for `#### {status}` followed by the first `Schema:`
/// ```yaml ... ``` ` fence.
pub fn parse_response_body_schema(markdown: &str, status: &str) -> Option<BodySchema> {
    let heading = format!("#### {}", status);
    let start = find_section(markdown, &heading)?;
    let slice = &markdown[start..];
    let bounded = bound_to_next_top_heading(slice);
    let yaml = extract_first_yaml_fence(bounded)?;
    Some(parse_schema_yaml(&yaml))
}

/// Extract the response `result:` payload shape — what the deployed
/// `*Response.cs` actually flattens into a class. The outer envelope
/// (`requestId`, `correlationId`, `responseId`, `dateTime`, `error`)
/// lives in `BaseResponseModel` and is not duplicated per endpoint.
///
/// The Dev Spec response section format used in this codebase:
///
/// ```text
/// ### Responses
///
/// #### Type: object        (synopsis — first appearance, often broken)
/// ...
///
/// **Schema**:
/// ```yaml
/// Type: object
/// Properties:
///   - requestId (required): …
///   - correlationId (required): …
///   - result:
///     Type: object
///     Description: Contains the response data
///     Properties:
///       id: …              (sometimes missing leading `- `)
///       - name: …
///       - currency_code: …
///   - error: …
/// ```
/// ```
///
/// Walk every `### Responses` section, find the first `**Schema**:`
/// fence under it, then drill into `- result:` for the payload.
///
/// Returns `(BodySchema, is_list)` where `is_list` is true when the
/// result is `Type: array`. For list shapes the inner item
/// Properties are returned and the caller wraps as `List<TItem>`.
pub fn parse_response_result_schema(markdown: &str) -> Option<(BodySchema, bool)> {
    // Try response section anchors in priority order. Some Dev Spec
    // files have `### Responses` then `#### 200/201` inside;
    // others go straight to `#### 201` after `### Request Body`. We
    // walk both, picking the first one that yields a parseable payload.
    let anchors = ["### Responses", "#### 201", "#### 200"];
    for anchor in anchors {
        let Some(start) = find_section(markdown, anchor) else {
            continue;
        };
        let slice = &markdown[start..];
        // For `#### 201` / `#### 200`, bound at the NEXT same-level
        // or higher heading so we don't drift into the `#### 400`
        // error section. `### Responses` bounds at the next `###`.
        let bounded = bound_to_next_response_section(slice, anchor);

        let mut cursor = 0usize;
        while cursor < bounded.len() {
            let Some(fence_rel) = bounded[cursor..].find("```yaml") else {
                break;
            };
            let fence_abs = cursor + fence_rel;
            let after_open = fence_abs + "```yaml".len();
            let body_start = match bounded[after_open..].find('\n') {
                Some(n) => after_open + n + 1,
                None => break,
            };
            let body = &bounded[body_start..];
            let stop = match body.find("```") {
                Some(c) => match next_heading_line(body) {
                    Some(h) => c.min(h),
                    None => c,
                },
                None => match next_heading_line(body) {
                    Some(h) => h,
                    None => body.len(),
                },
            };
            let yaml = &body[..stop];
            if let Some(payload) = extract_result_payload_from_yaml(yaml) {
                if !payload.0.fields.is_empty() {
                    return Some(payload);
                }
            }
            cursor = body_start + stop;
        }
    }
    None
}

/// Bound the search slice to the end of this response section. For
/// `### Responses`, stop at the next `### ` or `## ` heading. For
/// `#### 201` / `#### 200`, stop at the next `#### ` (sibling error
/// section like `#### 400`) or shallower heading.
fn bound_to_next_response_section<'a>(slice: &'a str, anchor: &str) -> &'a str {
    let after_first = match slice.find('\n') {
        Some(i) => i + 1,
        None => return slice,
    };
    let body = &slice[after_first..];
    let needles: &[&str] = if anchor.starts_with("####") {
        &["\n#### ", "\n### ", "\n## ", "\n# "]
    } else {
        &["\n### ", "\n## ", "\n# "]
    };
    let mut earliest = body.len();
    for n in needles {
        if let Some(idx) = body.find(n) {
            if idx < earliest {
                earliest = idx;
            }
        }
    }
    &slice[..after_first + earliest]
}

/// Same as `parse_response_result_schema` but takes pre-extracted YAML
/// directly — used by the walker above.
fn extract_result_payload_from_yaml(yaml: &str) -> Option<(BodySchema, bool)> {

    // Walk the yaml line-by-line. When we hit `- result:` capture its
    // indent. Then collect every following line that's more indented
    // than the result line until we leave the result block.
    let lines: Vec<&str> = yaml.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        if trimmed.starts_with("- result:") || trimmed.starts_with("- result :") {
            break;
        }
        i += 1;
    }
    if i >= lines.len() {
        return None;
    }
    let result_indent = lines[i].len() - lines[i].trim_start().len();
    let mut is_list = false;
    let mut payload = String::new();
    let mut in_properties = false;
    let mut props_indent: Option<usize> = None;
    i += 1;
    while i < lines.len() {
        let line = lines[i];
        let trimmed_l = line.trim_start();
        let indent = line.len() - trimmed_l.len();
        // Stop when we hit a sibling list item at the result indent or
        // shallower.
        if !trimmed_l.is_empty() && indent <= result_indent {
            break;
        }
        // List detection: only `Type:` lines that are direct children
        // of the `- result:` block count. Indented further means we're
        // inside a nested object — those `Type: array`s belong to
        // sub-fields like `accountMembers`, not to `result` itself.
        if !is_list && trimmed_l.starts_with("Type:") && indent == result_indent + 2 {
            let v = trimmed_l[5..].trim();
            if v.eq_ignore_ascii_case("array") {
                is_list = true;
            }
        }
        if trimmed_l.starts_with("Properties:") {
            in_properties = true;
            props_indent = Some(indent);
            i += 1;
            continue;
        }
        if in_properties {
            if let Some(p_indent) = props_indent {
                if !trimmed_l.is_empty() && indent < p_indent {
                    in_properties = false;
                    continue;
                }
            }
            let strip = props_indent.unwrap_or(0).saturating_add(2);
            let to_push = if line.len() >= strip {
                &line[strip..]
            } else {
                trimmed_l
            };
            // Repair Dev Spec defect: the first property under
            // `Properties:` sometimes appears without the leading `- `
            // marker (e.g. `id:` instead of `- id:`). Detect a header-
            // shaped line (identifier followed by colon, no leading `-`)
            // and inject the dash so the inner parser sees a uniform
            // field list.
            let normalised = if !to_push.starts_with("- ")
                && to_push.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && to_push.contains(':')
            {
                format!("- {}", to_push)
            } else {
                to_push.to_string()
            };
            payload.push_str(&normalised);
            payload.push('\n');
        }
        i += 1;
    }

    if payload.trim().is_empty() {
        return None;
    }
    let synthetic = format!(
        "Type: object\nProperties:\n{}",
        reindent_for_parse(&payload)
    );
    Some((parse_schema_yaml(&synthetic), is_list))
}

/// Re-indent the result payload so each `- name:` line starts at col 2,
/// matching what `parse_schema_yaml` expects after `Properties:`.
fn reindent_for_parse(payload: &str) -> String {
    let mut out = String::with_capacity(payload.len());
    for line in payload.lines() {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn find_section(markdown: &str, heading: &str) -> Option<usize> {
    let mut search_from = 0usize;
    while search_from < markdown.len() {
        let rest = &markdown[search_from..];
        let idx = rest.find(heading)?;
        let abs = search_from + idx;
        let is_line_start = abs == 0 || markdown.as_bytes()[abs - 1] == b'\n';
        if is_line_start {
            return Some(abs);
        }
        search_from = abs + heading.len();
    }
    None
}

fn extract_first_yaml_fence(slice: &str) -> Option<String> {
    let open = slice.find("```yaml")?;
    let after_open_start = open + "```yaml".len();
    let after_open = &slice[after_open_start..];
    let newline = after_open.find('\n')?;
    let body_start = after_open_start + newline + 1;
    let body = &slice[body_start..];

    // Find the literal closing fence ```...
    let close_rel = body.find("```");

    // Defensive guard: some Dev Spec files have unclosed yaml fences
    // (the request-body fence runs into the next `#### NNN` response
    // section without a closing ```). Treat the next heading line as
    // an implicit end-of-fence so the request schema parser doesn't
    // bleed response-envelope fields into the request DTO.
    let implicit_close = next_heading_line(body);

    let stop = match (close_rel, implicit_close) {
        (Some(c), Some(i)) => c.min(i),
        (Some(c), None) => c,
        (None, Some(i)) => i,
        (None, None) => return None,
    };
    Some(body[..stop].to_string())
}

/// Position (within `body`) of the next line that starts with a `#`
/// heading marker. Returns `None` if no heading appears.
fn next_heading_line(body: &str) -> Option<usize> {
    // Check if the body itself starts with a heading.
    if body.starts_with("#### ") || body.starts_with("### ") || body.starts_with("## ") {
        return Some(0);
    }
    let needles = ["\n#### ", "\n### ", "\n## "];
    let mut best: Option<usize> = None;
    for n in needles {
        if let Some(idx) = body.find(n) {
            // Skip the leading \n so the YAML body ends mid-line.
            best = Some(best.map_or(idx + 1, |b| b.min(idx + 1)));
        }
    }
    best
}

fn parse_schema_yaml(yaml: &str) -> BodySchema {
    let mut out = BodySchema::default();
    let lines: Vec<&str> = yaml.lines().collect();
    let mut i = 0usize;

    // Top-level `Required: a, b, c` line (optional).
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Required:") {
            out.required_top_level = rest
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if trimmed.starts_with("Properties:") {
            i += 1;
            break;
        }
        i += 1;
    }
    if i >= lines.len() {
        return out;
    }

    // Each field starts with `  - <name>(opt-suffix):` at some
    // indentation. Subsequent indented lines (`Type:`, `Description:`,
    // `Format:`, ...) belong to that field until the next `- ` marker
    // at the same or shallower indent. `Items:` is a nested block we
    // pick `Type:` + `Format:` out of.
    let mut current: Option<BodyField> = None;
    let mut in_items = false;
    while i < lines.len() {
        let raw = lines[i];
        let trimmed = raw.trim_start();
        i += 1;

        if trimmed.is_empty() {
            continue;
        }

        if let Some(after_dash) = trimmed.strip_prefix("- ") {
            // New field.
            if let Some(prev) = current.take() {
                out.fields.push(prev);
            }
            in_items = false;
            let (name, required) = split_field_header(after_dash);
            current = Some(BodyField {
                name,
                required,
                ..Default::default()
            });
            continue;
        }

        let Some(field) = current.as_mut() else {
            continue;
        };

        if trimmed.starts_with("Items:") {
            in_items = true;
            continue;
        }

        let (k, v) = match split_kv(trimmed) {
            Some(kv) => kv,
            None => continue,
        };

        if in_items {
            match k.as_str() {
                "Type" => field.items_type = v,
                "Format" => field.items_format = v,
                _ => {}
            }
            continue;
        }

        match k.as_str() {
            "Type" => field.yaml_type = v,
            "Format" => field.format = v,
            "Description" => field.description = v,
            "Example" => field.example = v,
            "Enum" => {
                field.enums = v
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "Max Length" => field.max_length = v,
            _ => {}
        }
    }
    if let Some(prev) = current.take() {
        out.fields.push(prev);
    }
    // Drop entries that lack a `Type:` line — they're almost always
    // malformed Dev Spec rows: e.g. an `Enum: a, b, c` value mistakenly
    // formatted as nested `- a: <description>` lines (seen in
    // PUT-accounts-_account_id_-balance.md `operation` field).
    out.fields.retain(|f| !f.yaml_type.is_empty());
    // Cross-fill `required` from top-level list for any field whose
    // header didn't carry the `(required)` annotation.
    for f in out.fields.iter_mut() {
        if !f.required && out.required_top_level.iter().any(|n| n == &f.name) {
            f.required = true;
        }
    }
    out
}

fn split_field_header(after_dash: &str) -> (String, bool) {
    // Forms seen in the corpus:
    //   `id:`
    //   `name (required):`
    //   `accountOwnerId (required):`
    let colon = match after_dash.rfind(':') {
        Some(c) => c,
        None => return (after_dash.trim().to_string(), false),
    };
    let head = after_dash[..colon].trim();
    if let Some(paren) = head.find('(') {
        let name = head[..paren].trim();
        let suffix = &head[paren..];
        let required = suffix.to_lowercase().contains("required");
        return (name.to_string(), required);
    }
    (head.to_string(), false)
}

fn split_kv(line: &str) -> Option<(String, String)> {
    let colon = line.find(':')?;
    let k = line[..colon].trim().to_string();
    let v = line[colon + 1..].trim().to_string();
    if k.is_empty() {
        return None;
    }
    Some((k, v))
}

/// Map a Dev Spec field to its C# type. Order of precedence:
///   1. `Type: array` + `Items: Format: uuid` → `List<Guid>`
///   2. `Type: array` + `Items: Type: string` → `List<string>`
///   3. `Type: object` → `dynamic` (open shape, mirrors deployed pattern)
///   4. `Type: string` + `Format: uuid` → `Guid` (or `Guid?` when optional)
///   5. `Type: string` → `string`
///   6. `Type: integer` → `int` / `int?`
///   7. `Type: number` → `decimal` / `decimal?`
///   8. `Type: boolean` → `bool` / `bool?`
///
/// Strings carry their own null sentinel (a missing string is `null`), so
/// the optional marker `?` is only appended to value types.
pub fn cs_type_for(field: &BodyField) -> String {
    let optional = !field.required;
    let t = field.yaml_type.to_ascii_lowercase();
    let fmt = field.format.to_ascii_lowercase();
    let items_t = field.items_type.to_ascii_lowercase();
    let items_fmt = field.items_format.to_ascii_lowercase();

    if t == "array" {
        if items_fmt == "uuid" || items_t == "uuid" {
            return "List<Guid>".to_string();
        }
        if items_t == "string" {
            return "List<string>".to_string();
        }
        if items_t == "integer" {
            return "List<int>".to_string();
        }
        return "List<object>".to_string();
    }
    if t == "object" {
        return "dynamic".to_string();
    }
    if t == "string" {
        if fmt == "uuid" {
            return if optional { "Guid?".to_string() } else { "Guid".to_string() };
        }
        return "string".to_string();
    }
    if t == "integer" {
        return if optional { "int?".to_string() } else { "int".to_string() };
    }
    if t == "number" {
        return if optional { "decimal?".to_string() } else { "decimal".to_string() };
    }
    if t == "boolean" {
        return if optional { "bool?".to_string() } else { "bool".to_string() };
    }
    "string".to_string()
}

/// snake_case → camelCase for the JSON wire field name.
///   `account_owner_id` → `accountOwnerId`
///   `currency_code`    → `currencyCode`
///   `accountOwnerId`   → `accountOwnerId` (unchanged)
///
/// This is the canonical wire representation per
/// `dtcard/.forge/openapi-standard.toml::body_field_casing = "camel_case"`.
/// The Dev Spec is allowed to author fields in snake_case (matches SQL
/// column names — single source of truth across SQL and Dev Spec); the
/// spec emitter and DTO renderer both rewrite to camelCase on the way
/// to anything client-facing.
pub fn camel_case(s: &str) -> String {
    if !s.contains('_') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut upper_next = false;
    let mut first = true;
    for c in s.chars() {
        if c == '_' {
            upper_next = true;
            continue;
        }
        if first {
            // First character stays lowercase.
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            first = false;
            continue;
        }
        if upper_next {
            for uc in c.to_uppercase() {
                out.push(uc);
            }
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// snake_case or camelCase → PascalCase for the C# property name.
///   `accountOwnerId`   → `AccountOwnerId`
///   `account_owner_id` → `AccountOwnerId`
///   `currency_code`    → `CurrencyCode`
///
/// Wire chain per openapi-standard.toml: snake_case (Dev Spec / SQL) →
/// camelCase (JSON over wire) → PascalCase (C# property). System.Text.Json
/// with `JsonNamingPolicy.CamelCase` handles PascalCase → camelCase
/// at serialisation; the C# property only needs to be PascalCase.
pub fn pascal_case(s: &str) -> String {
    if s.contains('_') {
        let mut out = String::with_capacity(s.len());
        let mut upper_next = true;
        for c in s.chars() {
            if c == '_' {
                upper_next = true;
                continue;
            }
            if upper_next {
                for uc in c.to_uppercase() {
                    out.push(uc);
                }
                upper_next = false;
            } else {
                out.push(c);
            }
        }
        return out;
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    if let Some(c) = chars.next() {
        for cc in c.to_uppercase() {
            out.push(cc);
        }
    }
    out.extend(chars);
    out
}

/// JSON example serialiser for the `<example>` tag body. Inputs from the
/// Dev Spec are already JSON-shaped (`["00000000-..."]`,
/// `{"k":"v"}`, `EUR`). We pass them through verbatim — the Dev Spec
/// author has already chosen the right form.
pub fn example_text(field: &BodyField) -> String {
    if !field.example.is_empty() {
        return field.example.clone();
    }
    // Sensible defaults per yaml type when the spec omitted Example.
    let cs = cs_type_for(field);
    match cs.as_str() {
        "Guid" | "Guid?" => "00000000-0000-0000-0000-000000000000".to_string(),
        "List<Guid>" => "[\"00000000-0000-0000-0000-000000000000\"]".to_string(),
        "decimal" | "decimal?" => "0.00".to_string(),
        "int" | "int?" => "0".to_string(),
        "bool" | "bool?" => "true".to_string(),
        "dynamic" => "{\"attributeName\":\"attributeValue\"}".to_string(),
        _ => String::new(),
    }
}

/// Dev-spec-derived map used by docs/audit. Field-name → C# type. Public
/// so the auditor can diff against deployed DTOs.
pub fn cs_type_map(schema: &BodySchema) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for f in &schema.fields {
        out.insert(pascal_case(&f.name), cs_type_for(f));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const POST_ACCOUNT_SCHEMA: &str = r#"
Type: object
Required: name, accountOwnerId
Properties:
  - id:
    Type: string
    Description: Unique ID for entity, Optional If omitted, random GUID assigned.
    Format: uuid
    Max Length: 36
  - name (required):
    Type: string
    Description: The official legal name of the business entity.
  - currencyCode:
    Type: string
    Description: ISO 4217 three-letter currency code.
    Example: EUR
  - accountMembers:
    Type: array
    Description: List of cardholder IDs.
    Items:
      Type: string
      Format: uuid
      Max Length: 36
  - accountOwnerId (required):
    Type: string
    Description: Primary cardholder.
    Format: uuid
    Max Length: 36
  - attributes:
    Type: object
"#;

    #[test]
    fn parses_request_body_schema_fields() {
        let s = parse_schema_yaml(POST_ACCOUNT_SCHEMA);
        let names: Vec<&str> = s.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["id", "name", "currencyCode", "accountMembers", "accountOwnerId", "attributes"]
        );
        assert!(s.fields[1].required); // name
        assert!(s.fields[4].required); // accountOwnerId
        assert!(!s.fields[0].required); // id
    }

    #[test]
    fn cs_types_resolve_per_field() {
        let s = parse_schema_yaml(POST_ACCOUNT_SCHEMA);
        let m = cs_type_map(&s);
        assert_eq!(m["Id"], "Guid?");
        assert_eq!(m["Name"], "string");
        assert_eq!(m["CurrencyCode"], "string");
        assert_eq!(m["AccountMembers"], "List<Guid>");
        assert_eq!(m["AccountOwnerId"], "Guid");
        assert_eq!(m["Attributes"], "dynamic");
    }

    #[test]
    fn pascal_case_camel_to_pascal() {
        assert_eq!(pascal_case("accountOwnerId"), "AccountOwnerId");
        assert_eq!(pascal_case("id"), "Id");
    }

    #[test]
    fn pascal_case_snake_to_pascal() {
        assert_eq!(pascal_case("account_owner_id"), "AccountOwnerId");
        assert_eq!(pascal_case("currency_code"), "CurrencyCode");
        assert_eq!(pascal_case("funding_type"), "FundingType");
        assert_eq!(pascal_case("attributes"), "Attributes");
    }

    #[test]
    fn response_result_list_field_resolves_to_list_guid() {
        // Mirrors GetAccountByIdResponse — `accountMembers` should
        // come out as a List<Guid>, not a single Guid?, after the
        // re-indent and inner-parser round-trip.
        let md = r#"
### Responses

**Schema**:
```yaml
Type: object
Properties:
  - result:
    Type: object
    Properties:
      id:
        Type: string
        Format: uuid
      - account_members:
        Type: array
        Description: members
        Items:
          Type: string
          Format: uuid
      - account_owner_id:
        Type: string
        Format: uuid
```
"#;
        let (s, is_list) = parse_response_result_schema(md).expect("schema");
        assert!(!is_list, "result is object, not array");
        let names: Vec<&str> = s.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["id", "account_members", "account_owner_id"]);
        let m = cs_type_map(&s);
        assert_eq!(m["Id"], "Guid?");
        assert_eq!(m["AccountMembers"], "List<Guid>", "array+uuid items → List<Guid>");
        assert_eq!(m["AccountOwnerId"], "Guid?");
    }

    #[test]
    fn camel_case_snake_to_camel() {
        assert_eq!(camel_case("account_owner_id"), "accountOwnerId");
        assert_eq!(camel_case("currency_code"), "currencyCode");
        assert_eq!(camel_case("accountOwnerId"), "accountOwnerId");
        assert_eq!(camel_case("id"), "id");
        assert_eq!(camel_case(""), "");
    }

    #[test]
    fn extracts_fence_from_request_body_section() {
        let md = format!(
            "## stuff\nignored\n### Request Body\n\nSchema:\n```yaml\n{}\n```\nafter\n",
            POST_ACCOUNT_SCHEMA.trim()
        );
        let s = parse_request_body_schema(&md).expect("schema");
        assert_eq!(s.fields.len(), 6);
    }
}
