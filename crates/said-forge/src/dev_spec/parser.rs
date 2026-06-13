//! Parse Dev Spec markdown files into `DevSpecEndpoint`. The format
//! has stable section anchors:
//!
//!   # <VERB> <path>                    ← H1: optional, fallback to filename
//!   **Summary**: <one-liner>
//!   ### Request Body                   ← Body section header
//!   ```yaml
//!   Type: object
//!   Properties:
//!     - <field>:                       ← parsed recursively
//!       Type: <type>
//!       ...
//!   ```
//!
//! The YAML schema block is hand-indented two-space, with leading `- `
//! before each property name. Standard serde_yaml choke on it because
//! it isn't valid YAML — keys appear to be sequence items. We parse
//! it line-by-line by indent level instead.

use crate::dev_spec::types::{DevSpecEndpoint, DevSpecParam, DevSpecSchema};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub fn parse_endpoint_file(
    path: &Path,
    standard: &crate::OpenApiStandard,
) -> Result<DevSpecEndpoint, String> {
    let mut rewrites = Vec::new();
    parse_endpoint_file_logged(path, standard, &mut rewrites)
}

/// Like `parse_endpoint_file`, but appends any pluralisation/path-param
/// rewrites to `rewrites` so the caller can surface them in `fixes.md`.
pub fn parse_endpoint_file_logged(
    path: &Path,
    standard: &crate::OpenApiStandard,
    rewrites: &mut Vec<PathRewriteLog>,
) -> Result<DevSpecEndpoint, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("no filename for {}", path.display()))?;
    let (method, raw_path) = method_path_from_filename(filename, standard)?;
    let (pluralised_path, plural_log) = pluralise_path(&method, &raw_path, standard);
    if let Some(reason) = plural_log {
        rewrites.push(PathRewriteLog {
            from: raw_path.clone(),
            to: pluralised_path.clone(),
            reason,
        });
    }
    // Apply the `paths.path_params.entity_id_canonicalisation` rule
    // from openapi-standard.toml: any generic `{id}` or snake-case
    // `{<entity>_id}` becomes `{<entity>Id}`, derived from the parent
    // path segment singularised + "Id". Runs AFTER pluralise so it sees
    // singular roots and produces stable canonical names.
    let (url_path, entity_id_log) = entity_id_canonicalise_path(&pluralised_path, standard);
    if let Some(reason) = entity_id_log {
        rewrites.push(PathRewriteLog {
            from: pluralised_path.clone(),
            to: url_path.clone(),
            reason,
        });
    }
    let summary = extract_summary(&text);
    let request_body = extract_schema_block(&text, "Request Body");
    // Response body: prefer the legacy `### Response Body` heading; fall
    // back to the modern `### Responses` → `#### 200`/`#### Type: object`
    // convention used by dtcard's Dev Spec markdown.
    let response_body = extract_schema_block(&text, "Response Body")
        .or_else(|| extract_response_success_schema(&text));
    let mut path_params = extract_path_params(&url_path);
    // Append query/header params declared in `### Parameters`. Header
    // params normally come from the global `parameters.standard_headers`
    // standard, so duplicates are filtered downstream by name+location.
    // Query params (page/limit/sort on GET-collection endpoints) are the
    // primary win here.
    for p in extract_parameters_section(&text) {
        path_params.push(p);
    }
    let mut ep = DevSpecEndpoint {
        method,
        path: url_path,
        summary,
        source_file: filename.to_string(),
        path_params,
        request_body,
        response_body,
    };
    normalise_body_field_casing(&mut ep, standard);
    strip_header_fields_from_request_body(&mut ep, standard);
    strip_id_for_modifying_verbs(&mut ep, standard);
    Ok(ep)
}

/// A path rewrite captured during parsing — surfaced by callers into
/// `fixes.md` via `FitReport.standard_normalisations`.
#[derive(Debug, Clone)]
pub struct PathRewriteLog {
    pub from: String,
    pub to: String,
    pub reason: String,
}

/// Filename convention: `<VERB>-<segment1>-<segment2>-...md`. Path
/// segments wrapped in `_..._` are template params.
pub fn method_path_from_filename(
    filename: &str,
    standard: &crate::OpenApiStandard,
) -> Result<(String, String), String> {
    let stem = filename.strip_suffix(".md").unwrap_or(filename);

    // Well-known endpoints carry a leading-dot path component which the
    // generic `-`-split parser would mangle (`.well` becomes a path
    // segment with a leading dot, then `known` becomes a separate
    // segment — wrong). Hard-coded here because OAuth/OIDC well-known
    // paths are an industry-standard URL structure that doesn't follow
    // dt's general filename convention.
    if let Some(rest) = stem.strip_prefix("GET-.well-known-") {
        // `GET-.well-known-openid-configuration` → path `/.well-known/openid-configuration`
        let tail = rest.replace('-', "/");
        return Ok(("GET".to_string(), format!("/.well-known/{}", tail)));
    }

    let mut parts = stem.split('-');
    let verb = parts.next().ok_or("filename empty")?;
    let verbs = ["GET", "POST", "PUT", "PATCH", "DELETE"];
    if !verbs.contains(&verb) {
        return Err(format!("unrecognised verb token in filename: {}", filename));
    }
    let mut path = String::new();
    let mut segments_emitted = 0usize;
    for raw in parts {
        // `createCardholder`-style trailing operation tokens are NOT path
        // segments — they're operation names. Filenames put them last
        // and they're always camelCase (no underscores), distinguishing
        // them from `_param_name_` placeholders. Heuristic: if the token
        // contains an uppercase letter and we've already emitted at
        // least one path segment, treat it as the operation name and stop.
        let is_camel_op = raw.chars().any(|c| c.is_ascii_uppercase())
            && !raw.starts_with('_');
        if is_camel_op && segments_emitted >= 1 {
            break;
        }
        let seg = if let Some(inner) = raw.strip_prefix('_').and_then(|s| s.strip_suffix('_')) {
            // Path params follow the BRU/OpenAPI standard: camelCase
            // ending in `Id` (e.g. accountId, businessId, transitionId).
            // Filename markers use snake_case (`_cardholder_id_`); convert
            // unless the standard says preserve as-is.
            match standard.paths.param_casing {
                crate::openapi_standard::ParamCasing::CamelCaseId =>
                    format!("{{{}}}", snake_to_camel_id(inner)),
                crate::openapi_standard::ParamCasing::Preserve =>
                    format!("{{{}}}", inner),
            }
        } else {
            raw.to_string()
        };
        path.push('/');
        path.push_str(&seg);
        segments_emitted += 1;
    }
    if path.is_empty() {
        return Err(format!("no path segments parsed from {}", filename));
    }
    Ok((verb.to_string(), path))
}

/// Apply the workspace pluralisation rule to a path. Returns
/// `(rewritten_path, rewrite_log)`. The log is `Some(reason)` only when
/// the path was actually modified, so callers can record it to
/// `fixes.md`.
///
/// Rule (`collection_plural_singular_id`):
///   - `GET /<collection>`            → plural (list all)
///   - `POST /<collection>`           → singular (create one)
///   - `<verb> /<root>/{id}/...`      → root segment singular
///   - C2 nested: nested `<sub>/{subId}` keeps `<sub>` plural
///   - `<verb> /<root>/{id}/<sub>`    → sub stays plural when bare
///   - `POST /<root>/{id}/<sub>`      → sub becomes singular (create one child)
pub fn pluralise_path(
    method: &str,
    path: &str,
    standard: &crate::OpenApiStandard,
) -> (String, Option<String>) {
    use crate::openapi_standard::CollectionPluralisation::*;
    if matches!(standard.paths.collection_pluralisation, Preserve) {
        return (path.to_string(), None);
    }

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return (path.to_string(), None);
    }

    let is_param = |s: &str| s.starts_with('{') && s.ends_with('}');
    let mut out: Vec<String> = Vec::with_capacity(segments.len());
    let verb = method.to_ascii_uppercase();

    for (i, seg) in segments.iter().enumerate() {
        if is_param(seg) {
            out.push(seg.to_string());
            continue;
        }
        let next = segments.get(i + 1).copied();
        let is_last = next.is_none();
        let next_is_id = next.map(is_param).unwrap_or(false);
        let is_root = i == 0;

        let want_singular = if is_root {
            // Root segment:
            //   POST /<root>            → singular
            //   POST /<root>/<anything> → singular (creating under one parent)
            //   <verb> /<root>/{id}/... → singular (resource by id)
            //   GET /<root>             → plural   (list all)
            //   GET /<root>/<sub>/...   → singular (you're addressing the
            //                             entity to reach a sub-resource,
            //                             not listing all entities)
            if verb == "POST" {
                true
            } else if next_is_id {
                true
            } else if is_last {
                false
            } else {
                // Root is followed by another non-param segment (a
                // sub-resource path like `/binsponsor/merchantcontrols/...`).
                // Per the C2 rule, the root is "this entity" (singular)
                // and the sub-resource collection stays plural further on.
                true
            }
        } else {
            // Nested segment: only flip when this is the LAST segment
            // and verb is POST (creating one child). Otherwise C2 keeps
            // child collections plural — including when followed by an
            // id (`/transitions/{transitionId}` stays plural).
            verb == "POST" && is_last
        };

        let rewritten = if want_singular {
            singularise(seg)
        } else if is_root && is_last {
            // GET /<root> with no id after → must be plural for the
            // collection-list rule. Source can be singular (`/binsponsor`)
            // — actively pluralise.
            pluralise_segment(seg)
        } else {
            seg.to_string()
        };
        out.push(rewritten);
    }

    let new_path = format!("/{}", out.join("/"));
    if new_path == path {
        (new_path, None)
    } else {
        // The reason string is also the section key in `fixes.md`,
        // so keep it stable across calls — don't bake verb/path into it.
        // The verb+path appear in the table row already.
        (
            new_path,
            Some("paths.collection_pluralisation = collection_plural_singular_id".to_string()),
        )
    }
}

/// Canonicalise generic `{id}` and snake-case `{<thing>_id}` to
/// entity-specific `{<entity>Id}`. Implements the
/// `paths.path_params.entity_id_canonicalisation = true` rule from
/// `openapi-standard.toml`.
///
/// Examples (with `{ default }` standard):
///   /products/{id}                     → /products/{productId}
///   /products/{product_id}             → /products/{productId}
///   /businesses/{business_id}          → /businesses/{businessId}
///   /cardholders/{cardholder_id}/cards → /cardholders/{cardholderId}/cards
///   /cards/{cardId}                    → /cards/{cardId}    (already canonical)
///
/// Entity = the LAST non-param segment before the param. Singularised
/// + camelCased + "Id" suffix.
///
/// Returns `(rewritten_path, reason_log)` where the log is `Some` only
/// when the path was actually modified, mirroring `pluralise_path`.
pub fn entity_id_canonicalise_path(
    path: &str,
    standard: &crate::OpenApiStandard,
) -> (String, Option<String>) {
    if !standard.parameters.path_params.entity_id_canonicalisation {
        return (path.to_string(), None);
    }
    if !path.starts_with('/') {
        return (path.to_string(), None);
    }
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return (path.to_string(), None);
    }
    let is_param = |s: &str| s.starts_with('{') && s.ends_with('}');
    let mut out: Vec<String> = Vec::with_capacity(segments.len());
    let mut last_entity_seg: Option<&str> = None;
    for seg in &segments {
        if is_param(seg) {
            let inner = &seg[1..seg.len() - 1];
            if let Some(entity) = last_entity_seg {
                let root = singularise(&entity.to_lowercase());
                let expected_camel = format!("{}Id", root);
                let expected_lc = expected_camel.to_lowercase();
                let inner_lc = inner.to_lowercase();
                let inner_is_id = inner == "id";
                let inner_is_snake_entity_id = inner.ends_with("_id");
                let inner_already_canonical = inner_lc == expected_lc;
                if (inner_is_id || inner_is_snake_entity_id) && !inner_already_canonical {
                    out.push(format!("{{{}}}", expected_camel));
                    continue;
                }
            }
            out.push(seg.to_string());
        } else {
            last_entity_seg = Some(*seg);
            out.push(seg.to_string());
        }
    }
    let new_path = format!("/{}", out.join("/"));
    if new_path == path {
        (new_path, None)
    } else {
        (new_path, Some(
            "parameters.path_params.entity_id_canonicalisation = true".to_string(),
        ))
    }
}

/// Convert a plural English noun to its singular form using the three
/// regular patterns we encounter in REST resource names: `-ies` → `-y`,
/// `-es` → `` (after `s`/`x`/`ch`/`sh`), `-s` → ``. Words that don't
/// look plural are returned unchanged.
pub fn singularise(s: &str) -> String {
    if s.len() < 2 {
        return s.to_string();
    }
    // Acronyms and segments containing digits (e.g. `3ds`, `oauth2`,
    // `iso8601`) are not English plurals — leave them alone. Anything
    // with at least one digit gets exempted; this catches `3ds`,
    // `aml5`, etc. without a hand-curated allow-list.
    if s.chars().any(|c| c.is_ascii_digit()) {
        return s.to_string();
    }
    let lower = s.to_ascii_lowercase();
    if let Some(stem) = lower.strip_suffix("ies") {
        // categories → category, currencies → currency
        return format!("{}y", stem);
    }
    if let Some(stem) = lower.strip_suffix("es") {
        // businesses → business, addresses → address, taxes → tax,
        // matches → match, dishes → dish.
        if stem.ends_with('s')
            || stem.ends_with('x')
            || stem.ends_with("ch")
            || stem.ends_with("sh")
        {
            return stem.to_string();
        }
        // Otherwise the trailing `e` is part of the stem (e.g.
        // `transitiones` is not a word; `votes` → `vote`). Drop only
        // the `s`.
        return lower[..lower.len() - 1].to_string();
    }
    if let Some(stem) = lower.strip_suffix('s') {
        // accounts → account, transitions → transition, bins → bin.
        // Skip if word ends in `ss` (single-word: address, business)
        // — already handled by `-es` above; bare `-ss` words aren't
        // plural so we leave them unchanged.
        if stem.ends_with('s') {
            return s.to_string();
        }
        return stem.to_string();
    }
    s.to_string()
}

/// Inverse of `singularise`. Append the regular English plural suffix
/// when the segment looks singular: `-y` → `-ies`, `-s/-x/-ch/-sh` → `-es`,
/// otherwise `+s`. Acronyms (any digit) and already-plural words are
/// returned unchanged.
pub fn pluralise_segment(s: &str) -> String {
    if s.len() < 2 {
        return s.to_string();
    }
    if s.chars().any(|c| c.is_ascii_digit()) {
        return s.to_string();
    }
    let lower = s.to_ascii_lowercase();
    // Already plural? (Round-trip via singularise; if singularise
    // produces something different the input was plural.)
    if singularise(&lower) != lower {
        return s.to_string();
    }
    if let Some(stem) = lower.strip_suffix('y') {
        // category → categories. But not if preceded by a vowel (key → keys).
        if let Some(prev) = stem.chars().last() {
            if !"aeiou".contains(prev) {
                return format!("{}ies", stem);
            }
        }
    }
    if lower.ends_with('s')
        || lower.ends_with('x')
        || lower.ends_with("ch")
        || lower.ends_with("sh")
    {
        return format!("{}es", lower);
    }
    format!("{}s", lower)
}

/// Normalise a snake_case path-param token to camelCase ending in `Id`.
/// `cardholder_id` → `cardholderId`, `transition_id` → `transitionId`,
/// `account_id` → `accountId`. Tokens without trailing `_id` get
/// camelCased on internal underscores only (e.g. `card_holder` →
/// `cardHolder`).
fn snake_to_camel_id(s: &str) -> String {
    snake_to_camel(s)
}

/// Convert a snake_case identifier to camelCase. Already-camelCase or
/// PascalCase strings are returned unchanged. Used for both path-param
/// names (`account_id` → `accountId`) and body field keys
/// (`first_name` → `firstName`).
pub fn snake_to_camel(s: &str) -> String {
    // Snake-case detection: must contain at least one underscore between
    // lowercase letters. Strings without underscores are returned as-is
    // so PascalCase or already-camelCase identifiers aren't damaged.
    if !s.contains('_') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut upper_next = false;
    for c in s.chars() {
        if c == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            out.extend(c.to_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Normalise body field casing across the endpoint per
/// `parameters.body_field_casing`. Walks both request and response
/// bodies recursively, rewriting object keys, and also rewrites
/// `path_params[].name` so all consumers see camelCase (S4).
pub fn normalise_body_field_casing(
    ep: &mut DevSpecEndpoint,
    standard: &crate::OpenApiStandard,
) {
    use crate::openapi_standard::BodyFieldCasing;
    if matches!(standard.parameters.body_field_casing, BodyFieldCasing::Preserve) {
        return;
    }
    if let Some(ref mut body) = ep.request_body {
        camelcase_keys_recursive(body);
    }
    if let Some(ref mut body) = ep.response_body {
        camelcase_keys_recursive(body);
    }
    for p in ep.path_params.iter_mut() {
        if p.name.contains('_') {
            p.name = snake_to_camel(&p.name);
        }
    }
}

/// Strip top-level keys from the request body that match a header
/// declared in `parameters.standard_headers.{required,optional}`, plus
/// always-response-only fields. Tracing concerns (`requestId`,
/// `correlationId`, `Ocp-Apim-Subscription-Key`, `x-api-version`)
/// belong in headers; envelope concerns (`result`, `error`, `dateTime`,
/// `responseId`) belong only in response bodies. Duplicating either
/// in the request body confuses generated clients. `id` is preserved —
/// it's a domain field for POST creates.
///
/// Response bodies are untouched (they keep these fields as the
/// envelope tracking fields).
///
/// Only top-level keys are stripped; nested object fields named the
/// same thing are preserved (they probably mean something different).
pub fn strip_header_fields_from_request_body(
    ep: &mut DevSpecEndpoint,
    standard: &crate::OpenApiStandard,
) {
    if !standard.parameters.strip_header_fields_from_request_body {
        return;
    }
    let body = match ep.request_body.as_mut() {
        Some(b) => b,
        None => return,
    };
    let headers = &standard.parameters.standard_headers;
    let mut forbidden: std::collections::HashSet<String> = std::collections::HashSet::new();
    for n in headers.required.iter().chain(headers.optional.iter()) {
        forbidden.insert(n.clone());
    }
    // Always-response-only fields. `responseId` is the unique-per-call
    // server stamp; `result`/`error`/`dateTime` are response-envelope
    // concerns. Regardless of header config or Dev Spec authoring
    // mistakes, none of these belong in a REQUEST body.
    for n in ["responseId", "result", "error", "dateTime"] {
        forbidden.insert(n.to_string());
    }
    body.properties.retain(|k, _| !forbidden.contains(k));
}

/// Strip `id` from request bodies for PUT/PATCH/DELETE operations.
/// The resource id lives in the URL path for these verbs; including
/// it in the body is redundant and creates ambiguity. POST is exempt
/// — POST creates a resource and the body's `id` is the new id.
pub fn strip_id_for_modifying_verbs(
    ep: &mut DevSpecEndpoint,
    standard: &crate::OpenApiStandard,
) {
    if !standard.parameters.strip_id_for_modifying_verbs {
        return;
    }
    let verb = ep.method.to_ascii_uppercase();
    if !matches!(verb.as_str(), "PUT" | "PATCH" | "DELETE") {
        return;
    }
    if let Some(body) = ep.request_body.as_mut() {
        body.properties.remove("id");
    }
}

/// Walk a schema in place and rewrite every object property key from
/// snake_case to camelCase. Items inside arrays (`items.properties`)
/// are also walked.
fn camelcase_keys_recursive(node: &mut DevSpecSchema) {
    let old = std::mem::take(&mut node.properties);
    let mut new_props = BTreeMap::new();
    for (k, mut v) in old {
        camelcase_keys_recursive(&mut v);
        let k_new = if k.contains('_') { snake_to_camel(&k) } else { k };
        new_props.insert(k_new, v);
    }
    node.properties = new_props;
    if let Some(items) = node.items.as_mut() {
        camelcase_keys_recursive(items);
    }
}

fn extract_summary(text: &str) -> String {
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("**Summary**:") {
            return rest.trim().to_string();
        }
    }
    String::new()
}

/// Parse `### Parameters` → `#### <name>` blocks from a Dev Spec
/// markdown file. Returns one `DevSpecParam` per H4 block with location
/// (query/header/path), required flag, type/default/min/max from the
/// fenced yaml `Schema:` block, plus optional example.
///
/// Output is filtered to query/header params; path params come from
/// `extract_path_params` (URL-derived) and stay distinct.
fn extract_parameters_section(text: &str) -> Vec<DevSpecParam> {
    let lines: Vec<&str> = text.lines().collect();
    // Find `### Parameters`.
    let mut i = 0usize;
    while i < lines.len() {
        let l = lines[i].trim();
        if l == "### Parameters" || l.starts_with("### Parameters") {
            break;
        }
        i += 1;
    }
    if i >= lines.len() {
        return Vec::new();
    }
    let mut out: Vec<DevSpecParam> = Vec::new();
    let mut j = i + 1;
    while j < lines.len() {
        let l = lines[j].trim();
        // Stop at the next `### `.
        if l.starts_with("### ") {
            break;
        }
        if !l.starts_with("#### ") {
            j += 1;
            continue;
        }
        // New parameter block. Walk forward until the next `####` or `###`.
        let name = l.trim_start_matches("#### ").trim().to_string();
        let mut p = DevSpecParam {
            name: name.clone(),
            ty: String::new(),
            required: false,
            description: String::new(),
            location: String::new(),
            default: None,
            minimum: None,
            maximum: None,
            example: None,
        };
        j += 1;
        while j < lines.len() {
            let l = lines[j].trim();
            if l.starts_with("#### ") || l.starts_with("### ") {
                break;
            }
            // Bullet lines: `- **Key**: value`
            if let Some(rest) = l.strip_prefix("- **") {
                if let Some(end) = rest.find("**:") {
                    let key = &rest[..end];
                    let value_raw = rest[end + 3..].trim();
                    let value = value_raw.trim_matches('`').to_string();
                    match key {
                        "Location" => p.location = value.to_lowercase(),
                        "Required" => p.required = matches!(
                            value.to_lowercase().as_str(),
                            "true" | "yes"
                        ),
                        "Description" => p.description = value,
                        "Example" => p.example = Some(value),
                        _ => {}
                    }
                }
                j += 1;
                continue;
            }
            // Fenced yaml schema block.
            if l == "```yaml" {
                let start = j + 1;
                let mut end = start;
                while end < lines.len() {
                    let m = lines[end].trim();
                    if m == "```" {
                        break;
                    }
                    if m.starts_with("# ")
                        || m.starts_with("## ")
                        || m.starts_with("### ")
                        || m.starts_with("#### ")
                        || m.starts_with("##### ")
                        || m == "```yaml"
                    {
                        break;
                    }
                    end += 1;
                }
                for line in &lines[start..end] {
                    let body = line.trim();
                    if let Some(v) = body.strip_prefix("Type:") {
                        p.ty = v.trim().to_lowercase();
                    } else if let Some(v) = body.strip_prefix("Default:") {
                        p.default = Some(v.trim().to_string());
                    } else if let Some(v) = body.strip_prefix("Minimum:") {
                        p.minimum = Some(v.trim().to_string());
                    } else if let Some(v) = body.strip_prefix("Maximum:") {
                        p.maximum = Some(v.trim().to_string());
                    }
                }
                j = end + 1;
                continue;
            }
            j += 1;
        }
        // Only keep query/header params here. Path params are sourced
        // separately from the URL template by `extract_path_params`.
        if matches!(p.location.as_str(), "query" | "header") && !p.name.is_empty() {
            out.push(p);
        }
    }
    out
}

fn extract_path_params(path: &str) -> Vec<DevSpecParam> {
    path.split('/')
        .filter_map(|seg| {
            seg.strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .map(|name| DevSpecParam {
                    // Path is already normalised in
                    // `method_path_from_filename`; keep names verbatim.
                    name: name.to_string(),
                    // Path params default to UUID for `*_id` and string otherwise.
                    // Refined later by reading the markdown's `### Parameters` block.
                    ty: if name.ends_with("_id") || name.ends_with("Id") {
                        "uuid".into()
                    } else {
                        "string".into()
                    },
                    required: true,
                    description: String::new(),
                    location: "path".into(),
                    default: None,
                    minimum: None,
                    maximum: None,
                    example: None,
                })
        })
        .collect()
}

/// Extract the success (200) response schema from a Dev Spec file
/// that uses the modern `### Responses` heading layout.
///
/// dtcard convention:
///   ### Responses
///   #### Type: object   <- success (200) block, schema in fenced yaml
///   #### 400
///   ...
///
/// Some files use `#### 200` instead of `#### Type: object`. Both are
/// recognised. Returns the first fenced yaml block found inside the
/// matched subsection.
fn extract_response_success_schema(text: &str) -> Option<DevSpecSchema> {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0usize;
    // Find `### Responses`.
    while i < lines.len() {
        let l = lines[i].trim();
        if l.starts_with("### ") && l.contains("Responses") {
            break;
        }
        i += 1;
    }
    if i >= lines.len() {
        return None;
    }
    // Inside the Responses block, find the FIRST `####` subsection that
    // looks like a 200/success block, then the yaml fence inside it.
    let mut j = i + 1;
    let mut in_success = false;
    while j < lines.len() {
        let l = lines[j].trim();
        if l.starts_with("### ") {
            // Walked into the next `###` section; stop.
            break;
        }
        if l.starts_with("#### ") {
            // Determine if this `####` is the 200/success block.
            let header = l.trim_start_matches("####").trim();
            in_success = matches!(header, "Type: object" | "200" | "Success" | "OK");
            j += 1;
            continue;
        }
        if !in_success {
            j += 1;
            continue;
        }
        if l == "```yaml" {
            let start = j + 1;
            let mut end = start;
            while end < lines.len() {
                let m = lines[end].trim();
                if m == "```" {
                    break;
                }
                if m.starts_with("# ")
                    || m.starts_with("## ")
                    || m.starts_with("### ")
                    || m.starts_with("#### ")
                    || m.starts_with("##### ")
                    || m == "```yaml"
                    || m.starts_with("```yaml")
                {
                    break;
                }
                end += 1;
            }
            if let Some(schema) = parse_schema_yaml(&lines[start..end]) {
                return Some(schema);
            }
            // Empty parse — keep scanning further `####` blocks.
            in_success = false;
        }
        j += 1;
    }
    None
}

/// Find the YAML schema block that follows `### <header_keyword>`
/// + a `**Schema**:` line. The yaml is fenced as ```yaml ... ```.
///
/// The schema block is bounded by the FIRST of:
///   - the closing ``` fence,
///   - a new markdown heading (`#`/`##`/`###`/`####`/...) — guards
///     against malformed Dev Spec where the closing fence was omitted
///     and the next section bleeds in,
///   - a fresh ```yaml opener (means the prior block was never closed
///     and a new one is starting).
fn extract_schema_block(text: &str, header_keyword: &str) -> Option<DevSpecSchema> {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with("### ") && line.contains(header_keyword) {
            // Look ahead for the next ```yaml block.
            let mut j = i + 1;
            while j < lines.len() {
                if lines[j].trim() == "```yaml" {
                    let start = j + 1;
                    let mut end = start;
                    while end < lines.len() {
                        let l = lines[end].trim();
                        if l == "```" {
                            break;
                        }
                        // Defensive bound: stop if we walk into the next
                        // markdown heading or a fresh fence opener.
                        // Malformed Dev Spec markdown sometimes omits the
                        // closing fence, which used to let the parser
                        // slurp the response envelope into the request
                        // body schema.
                        if l.starts_with("# ")
                            || l.starts_with("## ")
                            || l.starts_with("### ")
                            || l.starts_with("#### ")
                            || l.starts_with("##### ")
                            || l == "```yaml"
                            || l.starts_with("```yaml")
                        {
                            break;
                        }
                        end += 1;
                    }
                    if let Some(schema) = parse_schema_yaml(&lines[start..end]) {
                        return Some(schema);
                    }
                    // Yaml block produced nothing — keep scanning further
                    // headers in case there's a later matching section.
                    break;
                }
                // Stop at the next ### header to avoid skipping into the
                // wrong section.
                if lines[j].trim().starts_with("### ") && j > i {
                    break;
                }
                j += 1;
            }
        }
        i += 1;
    }
    None
}

/// Parse the indented "Schema" pseudo-yaml. Format:
///
///   Type: object
///   Properties:
///     - fieldA:
///       Type: string
///       Format: uuid
///     - fieldB:
///       Type: object
///       Properties:
///         - inner:
///           Type: integer
///
/// Indent unit is 2 spaces. Each property is `- name:` followed by
/// child key-value pairs at the next indent level.
fn parse_schema_yaml(lines: &[&str]) -> Option<DevSpecSchema> {
    let mut iter = lines.iter().peekable();
    parse_schema_node(&mut iter, 0)
}

fn parse_schema_node<'a, I: Iterator<Item = &'a &'a str>>(
    iter: &mut std::iter::Peekable<I>,
    indent: usize,
) -> Option<DevSpecSchema> {
    let mut node = DevSpecSchema {
        ty: String::new(),
        format: None,
        description: None,
        example: None,
        properties: BTreeMap::new(),
        items: None,
    };
    while let Some(line_ref) = iter.peek() {
        let line: &str = line_ref;
        if line.trim().is_empty() {
            iter.next();
            continue;
        }
        // Detect indent of this line. If it's less than expected, we've
        // exited this node's scope.
        let line_indent = line.chars().take_while(|c| *c == ' ').count();
        if line_indent < indent {
            break;
        }
        let body = line.trim_start();
        if body.starts_with("Type:") {
            node.ty = body.trim_start_matches("Type:").trim().to_lowercase();
            iter.next();
        } else if body.starts_with("Format:") {
            node.format = Some(body.trim_start_matches("Format:").trim().to_lowercase());
            iter.next();
        } else if body.starts_with("Description:") {
            node.description = Some(body.trim_start_matches("Description:").trim().to_string());
            iter.next();
        } else if body.starts_with("Example:") {
            node.example = Some(body.trim_start_matches("Example:").trim().to_string());
            iter.next();
        } else if body.starts_with("Properties:") {
            iter.next();
            // Children are `- name:` blocks at indent + 2.
            let child_indent = indent + 2;
            while let Some(peek) = iter.peek() {
                let peek_str: &str = peek;
                if peek_str.trim().is_empty() {
                    iter.next();
                    continue;
                }
                let peek_indent = peek_str.chars().take_while(|c| *c == ' ').count();
                let peek_body = peek_str.trim_start();
                if peek_indent < child_indent {
                    break;
                }
                // Accept `- name:` (canonical) or `name:` (defensive —
                // Dev Spec authors sometimes omit the dash). Reject any
                // line that's a known schema keyword at this indent
                // (would belong to the parent node, not a child).
                let is_known_keyword = peek_body.starts_with("Type:")
                    || peek_body.starts_with("Format:")
                    || peek_body.starts_with("Description:")
                    || peek_body.starts_with("Example:")
                    || peek_body.starts_with("Enum:")
                    || peek_body.starts_with("Items:")
                    || peek_body.starts_with("Required:")
                    || peek_body.starts_with("Properties:")
                    || peek_body.starts_with("Max Length:")
                    || peek_body.starts_with("Min Length:")
                    || peek_body.starts_with("Nullable:")
                    || peek_body.starts_with("Default:")
                    || peek_body.starts_with("Minimum:")
                    || peek_body.starts_with("Maximum:");
                if is_known_keyword {
                    // Belongs to the surrounding node — bail back to
                    // the outer parser loop.
                    break;
                }
                let name_line = if let Some(rest) = peek_body.strip_prefix("- ") {
                    rest
                } else if peek_body.ends_with(':') {
                    // Defensive: un-dashed `name:` at the right indent.
                    peek_body
                } else {
                    break;
                };
                // Strip optional ` (required)` / ` (optional)` annotations and trailing `:`.
                let raw_name = name_line.trim_end_matches(':').trim();
                let name = raw_name
                    .split_whitespace()
                    .next()
                    .unwrap_or(raw_name)
                    .to_string();
                iter.next();
                let child = parse_schema_node(iter, child_indent + 2)
                    .unwrap_or_else(|| DevSpecSchema {
                        ty: "string".into(),
                        format: None,
                        description: None,
                        example: None,
                        properties: BTreeMap::new(),
                        items: None,
                    });
                node.properties.insert(name, child);
            }
        } else if body.starts_with("Items:") {
            iter.next();
            if let Some(child) = parse_schema_node(iter, indent + 2) {
                node.items = Some(Box::new(child));
            }
        } else if body.starts_with("- ") {
            // Sibling property of a parent — bail.
            break;
        } else if body.contains(':') {
            // Unrecognised key (Example, Enum, Required, etc.) — skip.
            iter.next();
        } else {
            iter.next();
        }
    }
    if node.ty.is_empty() {
        None
    } else {
        Some(node)
    }
}

/// Recursively walk a Dev Spec directory, parsing every `*.md` whose
/// filename starts with a recognised HTTP verb. Files like `api.md`
/// (overview) and `README.md` are skipped — they don't follow the
/// `<VERB>-...md` contract.
///
/// Returns endpoints sorted by `(method, path)` for determinism.
pub fn walk_dev_spec_dir(
    root: &Path,
    standard: &crate::OpenApiStandard,
) -> Result<Vec<DevSpecEndpoint>, String> {
    let mut rewrites = Vec::new();
    walk_dev_spec_dir_logged(root, standard, &mut rewrites)
}

/// Like `walk_dev_spec_dir`, but appends every standard-driven path
/// rewrite to `rewrites`. CLI threads these into `fixes.md`.
pub fn walk_dev_spec_dir_logged(
    root: &Path,
    standard: &crate::OpenApiStandard,
    rewrites: &mut Vec<PathRewriteLog>,
) -> Result<Vec<DevSpecEndpoint>, String> {
    let mut out: Vec<DevSpecEndpoint> = Vec::new();
    walk_recursive(root, &mut out, standard, rewrites)?;
    out.sort_by(|a, b| (&a.method, &a.path).cmp(&(&b.method, &b.path)));
    Ok(out)
}

fn walk_recursive(
    dir: &Path,
    out: &mut Vec<DevSpecEndpoint>,
    standard: &crate::OpenApiStandard,
    rewrites: &mut Vec<PathRewriteLog>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("read_dir {}: {}", dir.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
        let path = entry.path();
        if path.is_dir() {
            walk_recursive(&path, out, standard, rewrites)?;
            continue;
        }
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !name.ends_with(".md") {
            continue;
        }
        let verbs = ["GET-", "POST-", "PUT-", "PATCH-", "DELETE-"];
        if !verbs.iter().any(|v| name.starts_with(v)) {
            continue;
        }
        let mut ep = parse_endpoint_file_logged(&path, standard, rewrites)?;
        // Decorate source_file with the parent-folder prefix so
        // downstream consumers (openapi tag derivation, story
        // attribution) can recover the domain. Falls back to
        // filename-only when parent dir name is unavailable.
        if let Some(parent_name) = path.parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
        {
            ep.source_file = format!("{}/{}", parent_name, ep.source_file);
        }
        out.push(ep);
    }
    Ok(())
}
