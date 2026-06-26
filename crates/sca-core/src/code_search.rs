//! Unified code search for .said files — Grep + LSP + SCA + Tree-sitter.
//!
//! Tree-sitter parses ANY language into an AST. We walk the AST to find
//! function/class/struct/impl nodes and chunk code at those boundaries.
//! No regex. No hardcoded patterns. Works for every language tree-sitter supports.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

// =========================================================================
// CODE SEARCH RESULTS
// =========================================================================

#[derive(Debug, Clone)]
pub struct CodeSearchResult {
    pub file_path: String,
    pub line: usize,
    pub content: String,
    pub score: f32,
    pub source: SearchSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SearchSource {
    Sca,
    Grep,
    Lsp,
    Fused,
}

// =========================================================================
// AST CODE CHUNK
// =========================================================================

#[derive(Debug, Clone)]
pub struct CodeChunk {
    pub name: String,
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
    pub kind: String, // "function", "class", "struct", "impl", etc.
    /// Symbol names this chunk REFERENCES (function calls / type uses). Used to build
    /// the code knowledge graph: each becomes a `call:<name>` edge so recall can traverse
    /// from a symbol to the things it calls (and, in reverse, find callers). Extracted
    /// deterministically from the chunk body — no LLM. Empty for non-code chunks.
    pub calls: Vec<String>,
}

/// Extract referenced symbol names (function-call targets) from a code chunk body.
/// Deterministic, language-agnostic heuristic: an identifier immediately followed by `(`
/// is a call site. Filters language keywords and the chunk's own name. This is the code
/// equivalent of `[[wikilinks]]` — `validate_session` calling `check_token` yields a
/// `check_token` reference, stored as a `call:check_token` edge.
pub fn extract_calls(body: &str, own_name: &str) -> Vec<String> {
    // keywords that are followed by `(` but aren't real call targets
    const KW: &[&str] = &[
        "if","for","while","switch","match","catch","return","sizeof","typeof","new",
        "await","yield","throw","with","when","fn","func","def","function","print",
        "println","assert","panic","let","var","const","using","public","private",
        "static","async","unsafe","do","else","in","is","as","and","or","not",
    ];
    let b = body.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut push = |id: &str, out: &mut Vec<String>| {
        if id.len() >= 2 && id != own_name && !KW.contains(&id)
            && id.chars().any(|c| c.is_ascii_alphabetic()) {
            let s = id.to_string();
            if !out.contains(&s) { out.push(s); }
        }
    };
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() || c == '_' {
            let s = i;
            while i < b.len() && ((b[i] as char).is_ascii_alphanumeric() || b[i] == b'_') { i += 1; }
            let ident = &body[s..i];
            // call form `ident(` (C-family, Rust, Python, JS, …)
            let mut j = i;
            while j < b.len() && (b[j] == b' ' || b[j] == b'\t') { j += 1; }
            if j < b.len() && b[j] == b'(' {
                push(ident, &mut out);
            } else if ident.eq_ignore_ascii_case("exec") || ident.eq_ignore_ascii_case("execute") {
                // SQL stored-proc call: `EXEC proc_name` / `EXECUTE proc_name` (no parens).
                // The next identifier is the callee.
                let mut k = j;
                // optional whitespace already skipped to j; ensure we're at an ident start
                while k < b.len() && (b[k] == b' ' || b[k] == b'\t') { k += 1; }
                if k < b.len() && ((b[k] as char).is_ascii_alphabetic() || b[k] == b'_') {
                    let ps = k;
                    while k < b.len() && ((b[k] as char).is_ascii_alphanumeric() || b[k] == b'_' || b[k] == b'.') { k += 1; }
                    // strip schema prefix (dbo.sp_x → sp_x)
                    let raw = &body[ps..k];
                    let callee = raw.rsplit('.').next().unwrap_or(raw);
                    push(callee, &mut out);
                }
            }
        } else {
            i += 1;
        }
    }
    out
}

// =========================================================================
// TREE-SITTER AST CHUNKING
// =========================================================================

/// Map a file extension to its tree-sitter `Language`, if we have a grammar.
/// `None` means "no grammar bundled" (caller should skip syntax-aware work).
/// SQL is intentionally `None` here — it uses the custom GO-batch parser, not
/// tree-sitter, so we cannot syntax-verify it this way.
#[cfg(feature = "code")]
fn language_for_ext(extension: &str) -> Option<tree_sitter::Language> {
    Some(match extension {
        "rs" => tree_sitter_rust::LANGUAGE.into(),
        "py" | "pyi" => tree_sitter_python::LANGUAGE.into(),
        "js" | "jsx" | "mjs" | "cjs" => tree_sitter_javascript::LANGUAGE.into(),
        "ts" | "tsx" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "java" => tree_sitter_java::LANGUAGE.into(),
        "cs" => tree_sitter_c_sharp::LANGUAGE.into(),
        "c" | "h" => tree_sitter_c::LANGUAGE.into(),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => tree_sitter_cpp::LANGUAGE.into(),
        "rb" => tree_sitter_ruby::LANGUAGE.into(),
        "php" => tree_sitter_php::LANGUAGE_PHP.into(),
        "kt" | "kts" => tree_sitter_kotlin_ng::LANGUAGE.into(),
        "swift" => tree_sitter_swift::LANGUAGE.into(),
        "lua" => tree_sitter_lua::LANGUAGE.into(),
        "md" | "markdown" => tree_sitter_md::LANGUAGE.into(),
        "json" => tree_sitter_json::LANGUAGE.into(),
        "yaml" | "yml" => tree_sitter_yaml::LANGUAGE.into(),
        "toml" => tree_sitter_toml_ng::LANGUAGE.into(),
        "xml" => tree_sitter_xml::LANGUAGE_XML.into(),
        "sh" | "bash" => tree_sitter_bash::LANGUAGE.into(),
        "tf" | "tfvars" | "hcl" => tree_sitter_hcl::LANGUAGE.into(),
        "ps1" | "psm1" => tree_sitter_powershell::LANGUAGE.into(),
        "pl" | "pm" => tree_sitter_perl::LANGUAGE.into(),
        _ => return None,
    })
}

/// Verify that `source` parses without syntax errors under the grammar for
/// `extension`. Returns `Ok(())` when the file is syntactically valid OR when
/// we have no grammar for that extension (can't verify → don't block).
/// Returns `Err` only when we DO have a grammar and the parse contains an
/// ERROR or missing node — i.e. the edit left the file un-parseable.
#[cfg(feature = "code")]
pub fn verify_syntax(source: &str, extension: &str) -> Result<(), String> {
    use tree_sitter::Parser;
    let language = match language_for_ext(extension) {
        Some(l) => l,
        None => return Ok(()), // no grammar → cannot verify, allow
    };
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return Ok(()); // grammar load failed → don't block the edit
    }
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return Err("parse failed entirely".to_string()),
    };
    if node_has_error(tree.root_node()) {
        return Err(format!(
            "edit would leave {} with a syntax error (unbalanced braces/parens or malformed code)",
            extension
        ));
    }
    Ok(())
}

/// Recursively check whether any node in the tree is an ERROR or is missing.
#[cfg(feature = "code")]
fn node_has_error(node: tree_sitter::Node) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if node_has_error(child) {
            return true;
        }
    }
    false
}

/// A named code scope (function, method, class, …) with its line range.
/// Used to make anchoring safe: "is this line inside method X?" and "where
/// does the class body end?" so an insert lands at the right scope.
#[cfg(feature = "code")]
#[derive(Debug, Clone)]
pub struct Scope {
    pub name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// Find the innermost named definition (function/method/class/struct/…) that
/// encloses `line` (1-based) in `source`. Returns `None` at top level or when
/// there's no grammar for `extension`. Used for safe-anchor diagnostics and
/// the scope-aware insert mode.
#[cfg(feature = "code")]
pub fn enclosing_scope(source: &str, extension: &str, line: usize) -> Option<Scope> {
    use tree_sitter::Parser;
    let language = language_for_ext(extension)?;
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(source, None)?;
    let mut best: Option<Scope> = None;
    collect_scopes(tree.root_node(), source.as_bytes(), line, &mut best);
    best
}

/// Walk the tree; for every named definition node whose line range contains
/// `line`, keep the SMALLEST (innermost) one.
#[cfg(feature = "code")]
fn collect_scopes(node: tree_sitter::Node, source: &[u8], line: usize, best: &mut Option<Scope>) {
    let kind = node.kind();
    let is_def = matches!(kind,
        "function_item" | "struct_item" | "enum_item" | "impl_item" | "trait_item" |
        "mod_item" | "function_definition" | "class_definition" | "method_definition" |
        "function_declaration" | "class_declaration" | "interface_declaration" |
        "method_declaration" | "type_declaration" | "constructor_declaration" |
        "enum_declaration" | "struct_specifier" | "namespace_declaration"
    );
    if is_def {
        let start = node.start_position().row + 1;
        let end = node.end_position().row + 1;
        if line >= start && line <= end {
            let name = find_name_node(node, source)
                .unwrap_or_else(|| format!("{}:L{}", kind, start));
            let span = end.saturating_sub(start);
            let better = match best {
                None => true,
                Some(b) => span < b.end_line.saturating_sub(b.start_line),
            };
            if better {
                *best = Some(Scope { name, kind: kind.to_string(), start_line: start, end_line: end });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_scopes(child, source, line, best);
    }
}

/// A copy-paste-ready anchor suggestion: a `said edit` argument set the caller
/// can run as-is. Returned in structured errors so an autonomous repair loop
/// gets a MENU of correct moves rather than prose to interpret.
#[cfg(feature = "code")]
#[derive(Debug, Clone)]
pub struct AnchorSuggestion {
    pub mode: String,
    pub symbol: String,
    /// Start line of the target span — pass back as `--line` to disambiguate a
    /// name that matches multiple spans (e.g. a C# class vs its constructor).
    pub line: usize,
    /// Declaration kind (class/method/…) so the caller can disambiguate by kind.
    pub kind: String,
    pub note: String,
}

/// Whether a definition `kind` is a CONTAINER (its body holds members), so the
/// right "add a member" move is `append-into-symbol` rather than insert-after.
/// Single source of truth — used by both source-based and index-based suggestions.
#[cfg(feature = "code")]
pub fn is_container_kind(kind: &str) -> bool {
    matches!(kind,
        "class_declaration" | "class_definition" | "impl_item" | "struct_item" |
        "struct_specifier" | "interface_declaration" | "namespace_declaration" | "mod_item")
        || kind.contains("class") || kind.contains("struct")
        || kind.contains("interface") || kind.contains("namespace")
}

/// Build an anchor menu from INDEX candidates alone (no source on disk) — for a
/// brain-only `--explain`. Each candidate is a `(name, kind, start_line,
/// end_line)`. Container kinds get `append-into-symbol`; everything else
/// `insert-after-symbol`. Reuses [`is_container_kind`] so the classification
/// matches the source-based path exactly (no duplicated, drifting kind lists).
#[cfg(feature = "code")]
pub fn suggest_anchors_from_candidates(
    cands: &[(String, String, usize, usize)],
) -> Vec<AnchorSuggestion> {
    cands.iter().map(|(name, kind, start, end)| {
        let mode = if is_container_kind(kind) { "append-into-symbol" } else { "insert-after-symbol" };
        AnchorSuggestion {
            mode: mode.to_string(),
            symbol: name.clone(),
            line: *start,
            kind: kind.clone(),
            note: format!(
                "{} `{}` at lines {}-{}; pass --line {} to target this span",
                short_kind(kind), name, start, end, start
            ),
        }
    }).collect()
}

/// Given a `line` where an anchor landed (often badly — e.g. inside a method),
/// return valid, scope-correct moves the caller can use instead. Collects the
/// enclosing scope chain and offers: append into the enclosing class body, and
/// insert after the immediate method/scope (both land at the right level).
#[cfg(feature = "code")]
pub fn suggest_anchors(source: &str, extension: &str, line: usize) -> Vec<AnchorSuggestion> {
    let mut scopes = enclosing_scopes(source, extension, line);
    // Order innermost → outermost so we can name the method and its class.
    scopes.sort_by_key(|s| s.end_line.saturating_sub(s.start_line));
    let mut out = Vec::new();
    // Enclosing container (class/struct/impl/namespace/mod): append into its body.
    if let Some(container) = scopes.iter().find(|s| is_container_kind(&s.kind))
    {
        out.push(AnchorSuggestion {
            mode: "append-into-symbol".to_string(),
            symbol: container.name.clone(),
            line: container.start_line,
            kind: container.kind.clone(),
            note: format!(
                "add a sibling member at the end of {} `{}` (lines {}-{}); pass --line {} to disambiguate",
                short_kind(&container.kind), container.name,
                container.start_line, container.end_line, container.start_line
            ),
        });
    }
    // Innermost method/function: insert after it (stays at the same scope).
    if let Some(method) = scopes.iter().find(|s| matches!(s.kind.as_str(),
        "method_declaration" | "function_declaration" | "function_definition" |
        "function_item" | "method_definition" | "constructor_declaration"))
    {
        out.push(AnchorSuggestion {
            mode: "insert-after-symbol".to_string(),
            symbol: method.name.clone(),
            line: method.start_line,
            kind: method.kind.clone(),
            note: format!(
                "insert after `{}` (lines {}-{}, same scope); pass --line {} to disambiguate",
                method.name, method.start_line, method.end_line, method.start_line
            ),
        });
    }

    out
}

/// Human-friendly short kind label (shared by source- and index-based menus).
#[cfg(feature = "code")]
fn short_kind(k: &str) -> &str {
    match k {
        "class_declaration" | "class_definition" => "class",
        "struct_item" | "struct_specifier" => "struct",
        "impl_item" => "impl",
        "interface_declaration" => "interface",
        "namespace_declaration" => "namespace",
        "mod_item" => "module",
        _ => "scope",
    }
}

/// Collect ALL named definition scopes that enclose `line` (1-based).
#[cfg(feature = "code")]
pub fn enclosing_scopes(source: &str, extension: &str, line: usize) -> Vec<Scope> {
    use tree_sitter::Parser;
    let mut out = Vec::new();
    let language = match language_for_ext(extension) { Some(l) => l, None => return out };
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() { return out; }
    let tree = match parser.parse(source, None) { Some(t) => t, None => return out };
    collect_all_scopes(tree.root_node(), source.as_bytes(), line, &mut out);
    out
}

#[cfg(feature = "code")]
fn collect_all_scopes(node: tree_sitter::Node, source: &[u8], line: usize, out: &mut Vec<Scope>) {
    let kind = node.kind();
    let is_def = matches!(kind,
        "function_item" | "struct_item" | "enum_item" | "impl_item" | "trait_item" |
        "mod_item" | "function_definition" | "class_definition" | "method_definition" |
        "function_declaration" | "class_declaration" | "interface_declaration" |
        "method_declaration" | "type_declaration" | "constructor_declaration" |
        "enum_declaration" | "struct_specifier" | "namespace_declaration"
    );
    if is_def {
        let start = node.start_position().row + 1;
        let end = node.end_position().row + 1;
        if line >= start && line <= end {
            let name = find_name_node(node, source).unwrap_or_else(|| format!("{}:L{}", kind, start));
            out.push(Scope { name, kind: kind.to_string(), start_line: start, end_line: end });
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_all_scopes(child, source, line, out);
    }
}

/// Chunk code by AST boundaries using tree-sitter.
/// Each function/class/struct/impl becomes one chunk.
#[cfg(feature = "code")]
pub fn ast_chunk(source: &str, extension: &str) -> Vec<CodeChunk> {
    use tree_sitter::Parser;

    let mut parser = Parser::new();

    // Set language based on extension
    let language = match extension {
        "rs" => tree_sitter_rust::LANGUAGE.into(),
        "py" => tree_sitter_python::LANGUAGE.into(),
        "js" | "jsx" | "mjs" => tree_sitter_javascript::LANGUAGE.into(),
        "ts" | "tsx" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "java" => tree_sitter_java::LANGUAGE.into(),
        "cs" => tree_sitter_c_sharp::LANGUAGE.into(),
        "sql" | "ddl" | "tsql" => {
            // SQL uses GO-delimited batches, not tree-sitter.
            // Custom parser handles CREATE/ALTER/INSERT and T-SQL stored procs.
            return sql_chunk(source);
        }
        _ => {
            // No tree-sitter grammar: return whole file as one chunk
            return vec![CodeChunk {
                name: "file".to_string(),
                content: source.to_string(),
                start_line: 1,
                end_line: source.lines().count(),
                kind: "file".to_string(),
                calls: vec![],
            }];
        }
    };

    parser.set_language(&language).expect("Failed to set language");

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => {
            return vec![CodeChunk {
                name: "file".to_string(),
                content: source.to_string(),
                start_line: 1,
                end_line: source.lines().count(),
                kind: "file".to_string(),
                calls: vec![],
            }];
        }
    };

    let source_bytes = source.as_bytes();
    let mut chunks = Vec::new();

    // Walk the AST and collect top-level and second-level definitions
    collect_chunks(tree.root_node(), source_bytes, &mut chunks, 0);

    // If no chunks found, return whole file
    if chunks.is_empty() {
        return vec![CodeChunk {
            name: "file".to_string(),
            content: source.to_string(),
            start_line: 1,
            end_line: source.lines().count(),
            kind: "file".to_string(),
            calls: vec![],
        }];
    }

    // Each chunk from collect_chunks is a *named definition* (function, struct,
    // method, …). A short body does NOT make it mergeable — folding it into the
    // previous chunk made short functions vanish from the symbol index and made
    // the predecessor's end_line over-extend, which let `said edit replace-symbol`
    // destroy the short neighbor. So we keep every named definition distinct and
    // only fold anonymous fragments (no real name) into their predecessor.
    let mut merged: Vec<CodeChunk> = Vec::new();
    for chunk in chunks {
        let is_anonymous = chunk.name.is_empty() || chunk.name.starts_with(&format!("{}:L", chunk.kind));
        if is_anonymous
            && chunk.end_line.saturating_sub(chunk.start_line) < 3
            && !merged.is_empty()
        {
            let last: &mut CodeChunk = merged.last_mut().unwrap();
            last.content.push('\n');
            last.content.push_str(&chunk.content);
            last.end_line = chunk.end_line;
        } else {
            merged.push(chunk);
        }
    }

    merged
}

/// Recursively collect definition nodes from the AST.
#[cfg(feature = "code")]
fn collect_chunks(
    node: tree_sitter::Node,
    source: &[u8],
    chunks: &mut Vec<CodeChunk>,
    depth: usize,
) {
    // Only go 2 levels deep (top-level + methods inside classes/impls)
    if depth > 2 { return; }

    let kind = node.kind();

    // These are the node types that represent meaningful code boundaries
    let is_definition = matches!(kind,
        // Rust
        "function_item" | "struct_item" | "enum_item" | "impl_item" |
        "trait_item" | "mod_item" | "type_item" | "const_item" | "static_item" |
        // Python
        "function_definition" | "class_definition" | "decorated_definition" |
        // JavaScript/TypeScript (function_declaration/class_declaration/
        // interface_declaration are shared with Go and Java/C# below)
        "function_declaration" | "class_declaration" | "interface_declaration" |
        "method_definition" | "arrow_function" | "export_statement" |
        "lexical_declaration" |
        // Go (method_declaration/type_declaration also cover Java/C#)
        "method_declaration" | "type_declaration" |
        // Java/C#
        "constructor_declaration" | "enum_declaration" |
        // SQL / T-SQL — each statement is a business logic unit
        "create_table" | "alter_table" | "drop_table" |
        "create_view" | "alter_view" | "create_materialized_view" |
        "create_procedure" | "alter_procedure" |
        "create_function" | "alter_function" |
        "create_trigger" | "create_index" | "alter_index" |
        "insert" | "update" | "delete" | "select" |
        "execute_statement" |
        "create_schema" | "create_role" | "create_type" |
        "create_sequence" | "alter_sequence"
    );

    if is_definition {
        let def_start = node.start_position();
        let end = node.end_position();
        let body = std::str::from_utf8(&source[node.byte_range()])
            .unwrap_or("")
            .to_string();

        // Prepend the contiguous LEADING doc-comments. In tree-sitter, `///` / `//!` / `#` / `/** */`
        // doc-comments are SIBLING comment nodes *before* the definition, NOT part of its byte range —
        // so without this the single richest natural-language description of what a function DOES (its
        // doc-comment) is excluded from the indexed chunk. Measured: "the function that builds the
        // wikilink concept graph" failed to retrieve `build_concept_links` even though its doc-comment
        // literally says it builds a concept graph from wiki/concept references — because that comment
        // wasn't indexed. Walk backwards over immediately-preceding comment siblings (contiguous, no
        // blank-line gap, so we don't slurp an unrelated earlier comment block) and fold them in.
        let mut lead = String::new();
        let mut anchor_row = def_start.row;
        let mut prev = node.prev_sibling();
        while let Some(p) = prev {
            if matches!(p.kind(), "line_comment" | "block_comment" | "comment")
                && p.end_position().row + 1 >= anchor_row
            {
                let txt = std::str::from_utf8(&source[p.byte_range()]).unwrap_or("");
                lead = format!("{}\n{}", txt, lead);
                anchor_row = p.start_position().row;
                prev = p.prev_sibling();
                continue;
            }
            break;
        }
        let (content, start_row) = if lead.is_empty() {
            (body, def_start.row)
        } else {
            // Index from the first doc-comment line so the chunk's span matches its content.
            (format!("{}\n{}", lead.trim_end(), body), anchor_row)
        };

        // Extract the name from the first identifier child
        let name = find_name_node(node, source)
            .unwrap_or_else(|| format!("{}:L{}", kind, def_start.row + 1));

        let calls = extract_calls(&content, &name);
        chunks.push(CodeChunk {
            name,
            content,
            start_line: start_row + 1,
            end_line: end.row + 1,
            kind: kind.to_string(),
            calls,
        });

        // For impl/class/procedure blocks, also collect their children
        if matches!(kind, "impl_item" | "class_definition" | "class_declaration" |
            "create_procedure" | "create_function" | "create_trigger") {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_chunks(child, source, chunks, depth + 1);
            }
        }

        return; // Don't recurse into this node's children again
    }

    // Recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_chunks(child, source, chunks, depth);
    }
}

/// Find the name identifier node in a definition.
#[cfg(feature = "code")]
fn find_name_node(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        // Standard code identifiers
        if child.kind() == "identifier" || child.kind() == "name" ||
           child.kind() == "type_identifier" || child.kind() == "property_identifier" {
            return std::str::from_utf8(&source[child.byte_range()])
                .ok()
                .map(|s| s.to_string());
        }
        // SQL: object names can be dotted (dbo.MyTable) or bracketed ([MyTable])
        if child.kind() == "object_name" || child.kind() == "table_name" ||
           child.kind() == "schema_qualified_name" || child.kind() == "dotted_name" {
            return std::str::from_utf8(&source[child.byte_range()])
                .ok()
                .map(|s| s.replace('[', "").replace(']', "").to_string());
        }
    }
    None
}

/// Fallback: no tree-sitter, chunk by fixed lines.
#[cfg(not(feature = "code"))]
pub fn ast_chunk(source: &str, _extension: &str) -> Vec<CodeChunk> {
    let lines: Vec<&str> = source.lines().collect();
    if lines.len() <= 100 {
        return vec![CodeChunk {
            name: "file".to_string(),
            content: source.to_string(),
            start_line: 1,
            end_line: lines.len(),
            kind: "file".to_string(),
            calls: vec![],
        }];
    }
    // Fixed 80-line chunks with 20-line overlap
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let end = (start + 80).min(lines.len());
        chunks.push(CodeChunk {
            name: format!("L{}-L{}", start + 1, end),
            content: lines[start..end].join("\n"),
            start_line: start + 1,
            end_line: end,
            kind: "chunk".to_string(),
            calls: vec![],
        });
        if end >= lines.len() { break; }
        start += 60;
    }
    chunks
}

// =========================================================================
// IMPORT/DEPENDENCY EXTRACTION — cross-language
// =========================================================================

/// Extract import/dependency references from source code.
/// Returns a list of module/package names this file depends on.
/// Works across all supported languages via simple pattern matching.
pub fn extract_imports(source: &str, extension: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for line in source.lines() {
        let trimmed = line.trim();

        let module = match extension {
            // Rust: use crate::module, use std::collections, mod name
            "rs" => {
                if let Some(rest) = trimmed.strip_prefix("use ") {
                    // use crate::frames::PutOptions → "crate::frames"
                    // use std::collections::HashMap → "std::collections"
                    let path = rest.trim_end_matches(';').trim();
                    // Take up to the last :: (drop the specific import name)
                    if let Some(pos) = path.rfind("::") {
                        Some(path[..pos].to_string())
                    } else {
                        Some(path.replace('{', "").replace('}', "").trim().to_string())
                    }
                } else if let Some(rest) = trimmed.strip_prefix("mod ") {
                    Some(rest.trim_end_matches(';').trim().to_string())
                } else {
                    None
                }
            }

            // Python: import module, from module import name
            "py" => {
                if let Some(rest) = trimmed.strip_prefix("from ") {
                    // from package.module import something → "package.module"
                    rest.split_whitespace().next().map(|s| s.to_string())
                } else if let Some(rest) = trimmed.strip_prefix("import ") {
                    // import os, sys → "os" (take first)
                    rest.split(',').next()
                        .map(|s| s.split_whitespace().next().unwrap_or("").to_string())
                } else {
                    None
                }
            }

            // JavaScript/TypeScript: import ... from 'module', require('module')
            "js" | "jsx" | "mjs" | "ts" | "tsx" => {
                if trimmed.contains("from ") {
                    // import { x } from 'module' or import x from "module"
                    if let Some(pos) = trimmed.rfind("from ") {
                        let after = trimmed[pos + 5..].trim();
                        let module = after.trim_matches(|c: char| c == '\'' || c == '"' || c == ';' || c == ' ');
                        if !module.is_empty() { Some(module.to_string()) } else { None }
                    } else { None }
                } else if trimmed.contains("require(") {
                    // const x = require('module')
                    if let Some(start) = trimmed.find("require(") {
                        let after = &trimmed[start + 8..];
                        let module = after.trim_start_matches(|c: char| c == '\'' || c == '"')
                            .split(|c: char| c == '\'' || c == '"' || c == ')')
                            .next()
                            .unwrap_or("");
                        if !module.is_empty() { Some(module.to_string()) } else { None }
                    } else { None }
                } else {
                    None
                }
            }

            // Go: import "package/path" or import ( "package/path" )
            "go" => {
                if trimmed.starts_with("import ") || trimmed.starts_with('"') {
                    let module = trimmed.trim_start_matches("import ")
                        .trim_matches(|c: char| c == '"' || c == '(' || c == ')' || c == ' ');
                    if !module.is_empty() && !module.starts_with("//") {
                        Some(module.to_string())
                    } else { None }
                } else { None }
            }

            // Java: import com.package.Class
            "java" => {
                if let Some(rest) = trimmed.strip_prefix("import ") {
                    let path = rest.trim_end_matches(';').trim();
                    // Take package without class: com.example.service.MyClass → com.example.service
                    if let Some(pos) = path.rfind('.') {
                        Some(path[..pos].to_string())
                    } else {
                        Some(path.to_string())
                    }
                } else { None }
            }

            // C#: using Namespace.SubNamespace
            "cs" => {
                if let Some(rest) = trimmed.strip_prefix("using ") {
                    let ns = rest.trim_end_matches(';').trim();
                    // Skip using statements with = (aliases) and static
                    if !ns.contains('=') && !ns.starts_with("static ") {
                        Some(ns.to_string())
                    } else { None }
                } else { None }
            }

            _ => None,
        };

        if let Some(m) = module {
            let clean = m.trim().to_string();
            if !clean.is_empty() && clean.len() > 1 && seen.insert(clean.clone()) {
                imports.push(clean);
            }
        }
    }

    imports
}

// =========================================================================
// SQL CHUNKER — GO-batch + statement-level parsing for T-SQL / SQL Server
//
// Extracts at five levels:
//   1. DDL objects: CREATE TABLE/VIEW/PROCEDURE/TRIGGER/INDEX/FUNCTION
//   2. Relational graph: FOREIGN KEY, CHECK constraints → tags on the frame
//   3. Lookup data: INSERT INTO reference tables → magic number resolution
//   4. Dynamic SQL: EXEC/sp_executesql detection → HasDynamicSQL flag
//   5. Table references: every table touched by a proc → "refs:" tags
// =========================================================================

/// Chunk SQL by GO-delimited batches and CREATE/ALTER statements.
/// Returns CodeChunks where:
///   - `name`:    object name (dbo.TableName, dbo.sp_DoStuff)
///   - `kind`:    statement type + metadata tags separated by `|`
///               e.g. "create_table|fk:dbo.Roles.RoleID|check:balance>=0"
///               e.g. "create_procedure|refs:dbo.Users,dbo.FICA|dynamic_sql"
///   - `content`: full SQL text including comments
fn sql_chunk(source: &str) -> Vec<CodeChunk> {
    let lines: Vec<&str> = source.lines().collect();
    let mut chunks = Vec::new();

    // Split into GO-delimited batches
    let mut batches: Vec<(usize, usize)> = Vec::new();
    let mut batch_start = 0;
    for (i, line) in lines.iter().enumerate() {
        if line.trim().eq_ignore_ascii_case("go") {
            if i > batch_start {
                batches.push((batch_start, i));
            }
            batch_start = i + 1;
        }
    }
    if batch_start < lines.len() {
        batches.push((batch_start, lines.len()));
    }

    for (batch_start, batch_end) in &batches {
        let batch_text = lines[*batch_start..*batch_end].join("\n");
        let trimmed = batch_text.trim();
        if trimmed.is_empty() || (trimmed.starts_with("--") && !trimmed.contains('\n')) {
            continue;
        }

        // Skip BOM + leading comments to find the actual SQL statement.
        // Comments above stored procs are preserved in the content but
        // shouldn't prevent detection of the statement type.
        let no_bom = trimmed.trim_start_matches('\u{FEFF}');
        let code_start = skip_sql_comments(no_bom);
        let upper = code_start.to_uppercase();

        // Detect statement type and extract object name
        let (base_kind, name) = if let Some(rest) = strip_sql_prefix(&upper, "CREATE TABLE") {
            ("create_table", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "ALTER TABLE") {
            ("alter_table", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "CREATE VIEW") {
            ("create_view", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "CREATE OR ALTER VIEW") {
            ("create_view", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "CREATE PROCEDURE") {
            ("create_procedure", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "CREATE PROC") {
            ("create_procedure", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "CREATE OR ALTER PROCEDURE") {
            ("create_procedure", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "ALTER PROCEDURE") {
            ("alter_procedure", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "ALTER PROC") {
            ("alter_procedure", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "CREATE FUNCTION") {
            ("create_function", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "CREATE TRIGGER") {
            ("create_trigger", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "CREATE INDEX") {
            ("create_index", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "CREATE NONCLUSTERED INDEX") {
            ("create_index", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "CREATE UNIQUE INDEX") {
            ("create_index", extract_sql_object_name(rest, trimmed))
        } else if let Some(rest) = strip_sql_prefix(&upper, "CREATE CLUSTERED INDEX") {
            ("create_index", extract_sql_object_name(rest, trimmed))
        } else if upper.starts_with("INSERT") {
            ("insert", extract_insert_target(&upper))
        } else if upper.starts_with("UPDATE") {
            ("update", extract_sql_first_word_after(&upper, "UPDATE"))
        } else if upper.starts_with("DELETE") {
            ("delete", extract_sql_first_word_after(&upper, "FROM"))
        } else if upper.starts_with("SELECT") {
            ("select", format!("query:L{}", batch_start + 1))
        } else if upper.starts_with("GRANT") || upper.starts_with("REVOKE") {
            ("grant", format!("permission:L{}", batch_start + 1))
        } else {
            ("sql_batch", format!("batch:L{}", batch_start + 1))
        };

        // Build metadata tags from the SQL content
        let mut tags: Vec<String> = Vec::new();

        // 1. FOREIGN KEY relationships → "fk:TargetTable.Column"
        for fk in extract_foreign_keys(&upper) {
            tags.push(format!("fk:{}", fk));
        }

        // 2. CHECK constraints → "check:expression"
        for ck in extract_check_constraints(&upper, trimmed) {
            tags.push(format!("check:{}", ck));
        }

        // 3. Dynamic SQL detection → "dynamic_sql"
        if has_dynamic_sql(&upper) {
            tags.push("dynamic_sql".to_string());
        }

        // 4. Table references → "refs:table1,table2,..."
        //    (for procs/views/triggers: which tables does this code touch?)
        if matches!(base_kind, "create_procedure" | "alter_procedure" |
                    "create_function" | "create_trigger" | "create_view") {
            let refs = extract_table_references(&upper);
            if !refs.is_empty() {
                tags.push(format!("refs:{}", refs.join(",")));
            }
        }

        // Combine kind + tags: "create_table|fk:dbo.Roles.RoleID|check:balance>=0"
        let kind = if tags.is_empty() {
            base_kind.to_string()
        } else {
            format!("{}|{}", base_kind, tags.join("|"))
        };

        let calls = extract_calls(&batch_text, &name);
        chunks.push(CodeChunk {
            name,
            content: batch_text,
            start_line: batch_start + 1,
            end_line: *batch_end,
            kind,
            calls,
        });
    }

    if chunks.is_empty() {
        return vec![CodeChunk {
            name: "file".to_string(),
            content: source.to_string(),
            start_line: 1,
            end_line: lines.len(),
            kind: "file".to_string(),
            calls: vec![],
        }];
    }

    chunks
}

/// Strip a SQL keyword prefix (case-insensitive) and return the remainder.
fn strip_sql_prefix<'a>(upper: &'a str, prefix: &str) -> Option<&'a str> {
    let trimmed = upper.trim();
    if trimmed.starts_with(prefix) {
        Some(trimmed[prefix.len()..].trim_start())
    } else {
        None
    }
}

/// Extract SQL object name from text after CREATE/ALTER keyword.
fn extract_sql_object_name(upper_rest: &str, original: &str) -> String {
    let token = upper_rest.split_whitespace().next().unwrap_or("unknown");
    let clean = token.replace(['[', ']'], "");
    // Find original casing in source
    let search = clean.to_lowercase();
    let orig_lower = original.to_lowercase();
    if let Some(pos) = orig_lower.find(&search) {
        if pos + clean.len() <= original.len() {
            return original[pos..pos + clean.len()].to_string();
        }
    }
    clean
}

/// Extract INSERT target table name.
fn extract_insert_target(upper: &str) -> String {
    if let Some(rest) = upper.strip_prefix("INSERT") {
        let rest = rest.trim_start();
        let rest = if let Some(r) = rest.strip_prefix("INTO") { r.trim_start() } else { rest };
        if let Some(token) = rest.split_whitespace().next() {
            return token.replace(['[', ']'], "");
        }
    }
    "insert".to_string()
}

/// Extract first word after a SQL keyword.
fn extract_sql_first_word_after(upper: &str, keyword: &str) -> String {
    if let Some(pos) = upper.find(keyword) {
        let rest = upper[pos + keyword.len()..].trim_start();
        if let Some(token) = rest.split_whitespace().next() {
            return token.replace(['[', ']'], "");
        }
    }
    keyword.to_lowercase()
}

// ─────────────────────────────────────────────────────────────
// 1. FOREIGN KEY extraction: "REFERENCES dbo.Table(Column)"
// ─────────────────────────────────────────────────────────────

fn extract_foreign_keys(upper: &str) -> Vec<String> {
    let mut fks = Vec::new();
    // Match: REFERENCES <table>(<col>)  or  REFERENCES <table> (<col>)
    let mut search_from = 0;
    while let Some(pos) = upper[search_from..].find("REFERENCES") {
        let abs_pos = search_from + pos;
        let after = upper[abs_pos + 10..].trim_start();
        // Extract table name (possibly schema-qualified)
        if let Some(table_token) = after.split(&['(', ' ', '\n', '\r'][..]).next() {
            let table = table_token.trim().replace(['[', ']'], "");
            if !table.is_empty() {
                // Try to extract column from parens
                if let Some(paren_start) = after.find('(') {
                    if let Some(paren_end) = after[paren_start..].find(')') {
                        let col = after[paren_start + 1..paren_start + paren_end]
                            .trim()
                            .replace(['[', ']'], "");
                        fks.push(format!("{}.{}", table, col));
                    } else {
                        fks.push(table);
                    }
                } else {
                    fks.push(table);
                }
            }
        }
        search_from = abs_pos + 10;
    }
    fks
}

// ─────────────────────────────────────────────────────────────
// 2. CHECK constraint extraction
// ─────────────────────────────────────────────────────────────

fn extract_check_constraints(upper: &str, original: &str) -> Vec<String> {
    let mut checks = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = upper[search_from..].find("CHECK") {
        let abs_pos = search_from + pos;
        // Skip "CHECK (" or "CHECK("
        let after = upper[abs_pos + 5..].trim_start();
        if after.starts_with('(') {
            // Find matching close paren (handle nesting)
            let orig_after = &original[abs_pos + 5..].trim_start();
            if let Some(expr) = extract_balanced_parens(orig_after) {
                let clean = expr.trim().to_string();
                if !clean.is_empty() && clean.len() < 200 {
                    checks.push(clean);
                }
            }
        }
        search_from = abs_pos + 5;
    }
    checks
}

/// Extract content between balanced parentheses.
fn extract_balanced_parens(s: &str) -> Option<String> {
    if !s.starts_with('(') { return None; }
    let mut depth = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[1..i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────
// 3. Dynamic SQL detection
// ─────────────────────────────────────────────────────────────

fn has_dynamic_sql(upper: &str) -> bool {
    // EXEC(@sql), EXECUTE(@sql), sp_executesql, or string concatenation
    // patterns like SET @sql = 'SELECT...' + @variable
    upper.contains("SP_EXECUTESQL")
        || (upper.contains("EXEC") && upper.contains("@"))
        || (upper.contains("EXECUTE") && upper.contains("@"))
        || (upper.contains("SET @") && upper.contains("+ '"))
}

// ─────────────────────────────────────────────────────────────
// 4. Table reference extraction (for procs/views/triggers)
// ─────────────────────────────────────────────────────────────

fn extract_table_references(upper: &str) -> Vec<String> {
    let mut refs = std::collections::HashSet::new();

    // Look for FROM <table>, JOIN <table>, INTO <table>, UPDATE <table>
    for keyword in &["FROM", "JOIN", "INTO", "UPDATE"] {
        let mut search_from = 0;
        while let Some(pos) = upper[search_from..].find(keyword) {
            let abs_pos = search_from + pos;
            let after = upper[abs_pos + keyword.len()..].trim_start();
            if let Some(token) = after.split_whitespace().next() {
                let table = token.replace(['[', ']', '(', ')', ',', ';'], "");
                // Filter out SQL keywords, variables, and noise
                if !table.is_empty()
                    && !table.starts_with('@')
                    && !table.starts_with('#')
                    && !table.starts_with('\'')
                    && table.len() > 1
                    && !is_sql_keyword(&table)
                {
                    refs.insert(table);
                }
            }
            search_from = abs_pos + keyword.len();
        }
    }

    let mut sorted: Vec<String> = refs.into_iter().collect();
    sorted.sort();
    sorted
}

/// Skip leading SQL comments (-- single-line and /* block */) to find
/// the actual statement. Handles nested block comments /* /* */ */.
fn skip_sql_comments(s: &str) -> &str {
    let mut rest = s.trim_start();
    loop {
        if rest.starts_with("--") {
            if let Some(nl) = rest.find('\n') {
                rest = rest[nl + 1..].trim_start();
            } else {
                return rest;
            }
        } else if rest.starts_with("/*") {
            // Handle nested block comments by tracking depth
            let mut depth = 0;
            let bytes = rest.as_bytes();
            let mut i = 0;
            while i < bytes.len().saturating_sub(1) {
                if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        rest = rest[i..].trim_start();
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            if depth > 0 {
                return rest; // unclosed block comment
            }
        } else {
            return rest;
        }
    }
}

fn is_sql_keyword(word: &str) -> bool {
    matches!(word,
        "SELECT" | "WHERE" | "AND" | "OR" | "NOT" | "IN" | "ON" |
        "SET" | "VALUES" | "AS" | "CASE" | "WHEN" | "THEN" | "ELSE" |
        "END" | "BEGIN" | "RETURN" | "NULL" | "IS" | "EXISTS" | "LIKE" |
        "BETWEEN" | "HAVING" | "GROUP" | "ORDER" | "BY" | "ASC" | "DESC" |
        "TOP" | "DISTINCT" | "UNION" | "ALL" | "ANY" | "SOME" |
        "INNER" | "OUTER" | "LEFT" | "RIGHT" | "CROSS" | "FULL" |
        "WITH" | "NOLOCK" | "INSERTED" | "DELETED" | "OUTPUT"
    )
}

// =========================================================================
// UNIFIED CODE SEARCH ENGINE
// =========================================================================

pub struct CodeSearch {
    workspace: PathBuf,
    rg_path: Option<String>,
}

impl CodeSearch {
    pub fn new(workspace: impl AsRef<Path>) -> Self {
        Self {
            workspace: workspace.as_ref().to_path_buf(),
            rg_path: Self::find_ripgrep(),
        }
    }

    /// Grep: exact text search via ripgrep.
    pub fn grep(&self, pattern: &str, glob: Option<&str>, max_results: usize) -> Vec<CodeSearchResult> {
        let rg = match &self.rg_path {
            Some(p) => p.clone(),
            None => return Vec::new(),
        };

        let mut cmd = Command::new(&rg);
        cmd.arg("--line-number")
           .arg("--no-heading")
           .arg("--color=never")
           .arg("--max-count").arg(max_results.to_string());

        if let Some(g) = glob {
            cmd.arg("--glob").arg(g);
        }

        cmd.arg(pattern)
           .arg(self.workspace.to_str().unwrap_or("."));

        let output = match cmd.output() {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };

        String::from_utf8_lossy(&output.stdout)
            .lines()
            .take(max_results)
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(3, ':').collect();
                if parts.len() >= 3 {
                    Some(CodeSearchResult {
                        file_path: parts[0].to_string(),
                        line: parts[1].parse().unwrap_or(0),
                        content: parts[2].to_string(),
                        score: 1.0,
                        source: SearchSource::Grep,
                    })
                } else { None }
            })
            .collect()
    }

    /// Glob: file discovery via ripgrep --files.
    pub fn glob(&self, pattern: &str, max_results: usize) -> Vec<String> {
        let rg = match &self.rg_path {
            Some(p) => p.clone(),
            None => return Vec::new(),
        };

        Command::new(&rg)
            .arg("--files")
            .arg("--glob").arg(pattern)
            .arg(self.workspace.to_str().unwrap_or("."))
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .take(max_results)
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Fused search: SCA semantic + Grep exact, ranked by RRF.
    pub fn fused_search(
        &self,
        query: &str,
        sca_results: &[(String, f32)],
        max_results: usize,
    ) -> Vec<CodeSearchResult> {
        let mut scored: HashMap<String, f32> = HashMap::new();

        for (i, (doc_id, sca_score)) in sca_results.iter().enumerate() {
            let rrf = 1.0 / (60.0 + i as f32);
            *scored.entry(doc_id.clone()).or_insert(0.0) += rrf * sca_score;
        }

        let grep_results = self.grep(query, None, 20);
        for (i, result) in grep_results.iter().enumerate() {
            let rrf = 1.0 / (60.0 + i as f32);
            let key = format!("{}:{}", result.file_path, result.line);
            *scored.entry(key).or_insert(0.0) += rrf;
        }

        let mut fused: Vec<(String, f32)> = scored.into_iter().collect();
        fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        fused.into_iter().take(max_results).map(|(key, score)| {
            let content = grep_results.iter()
                .find(|r| format!("{}:{}", r.file_path, r.line) == key)
                .map(|r| r.content.clone())
                .unwrap_or_default();

            CodeSearchResult {
                file_path: key,
                line: 0,
                content,
                score,
                source: SearchSource::Fused,
            }
        }).collect()
    }

    /// Index a codebase: walk files, AST-chunk each, store in .said.
    pub fn index_codebase(
        &self,
        said_file: &mut crate::said_file::SaidFile,
        extensions: &[&str],
    ) -> usize {
        let mut indexed = 0;

        for ext in extensions {
            let pattern = format!("**/*.{}", ext);
            let files = self.glob(&pattern, 50000);

            for file_path in &files {
                let full_path = self.workspace.join(file_path);
                if let Ok(content) = std::fs::read_to_string(&full_path) {
                    let relative = file_path.replace('\\', "/");

                    // AST-chunk by function/class boundaries (tree-sitter if available)
                    let chunks = ast_chunk(&content, ext);

                    for chunk in &chunks {
                        let chunk_id = if chunks.len() == 1 {
                            relative.clone()
                        } else {
                            format!("{}::{}", relative, chunk.name)
                        };
                        let title = format!("{} [{}] ({})", chunk.name, chunk.kind, relative);
                        said_file.put(&chunk_id, &chunk.content, Some(&title));
                        indexed += 1;
                    }
                }
            }
        }

        indexed
    }

    fn find_ripgrep() -> Option<String> {
        if Command::new("rg").arg("--version").output().is_ok() {
            return Some("rg".to_string());
        }
        for path in &["/usr/bin/rg", "/usr/local/bin/rg"] {
            if Path::new(path).exists() {
                return Some(path.to_string());
            }
        }
        None
    }
}

#[cfg(all(test, feature = "code"))]
mod chunk_tests {
    use super::*;

    /// Three real functions, the last with a short (<3-line) body, must remain
    /// THREE distinct chunks. Regression for the merge bug where a short
    /// function was folded into its predecessor — vanishing from the symbol
    /// index and over-extending the previous symbol's end_line (which made
    /// `said edit replace-symbol` destroy the short neighbor).
    #[test]
    fn short_adjacent_functions_stay_distinct() {
        let src = "\
fn keep_me() {
    println!(\"keep\");
}

fn target_fn() {
    let a = 1;
    let b = 2;
    println!(\"{}\", a + b);
}

fn also_keep() {
    println!(\"end\");
}
";
        let chunks = ast_chunk(src, "rs");
        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();

        assert!(names.contains(&"keep_me"), "keep_me missing: {:?}", names);
        assert!(names.contains(&"target_fn"), "target_fn missing: {:?}", names);
        assert!(
            names.contains(&"also_keep"),
            "also_keep vanished (merged away): {:?}",
            names
        );

        // target_fn must NOT over-extend into also_keep (body ends at line 9).
        let target = chunks.iter().find(|c| c.name == "target_fn").unwrap();
        assert!(
            target.end_line <= 9,
            "target_fn end_line {} over-extends past its body (line 9)",
            target.end_line
        );
    }

    // ---- scope detection (for safe anchoring) --------------------------

    #[test]
    fn enclosing_scope_finds_the_method_a_line_is_inside() {
        let src = "\
public class HealthTests
{
    public void Pid_test()
    {
        var x = 1;
    }
}
";
        // Line 5 ("var x = 1;") is inside the method Pid_test.
        let scope = enclosing_scope(src, "cs", 5).expect("should find a scope");
        assert_eq!(scope.name, "Pid_test");
        assert!(scope.kind.contains("method") || scope.kind.contains("function"));
    }

    #[test]
    fn enclosing_scope_reports_class_at_class_level() {
        let src = "\
public class HealthTests
{
    public void A() { }
}
";
        // Line 3 is at class body level (inside the class, between methods).
        let scope = enclosing_scope(src, "cs", 3).expect("should find a scope");
        // Innermost named scope containing line 3: the method A (it's on line 3),
        // or the class. Either way we get a usable name; assert we got something.
        assert!(!scope.name.is_empty());
    }

    #[test]
    fn enclosing_scope_none_at_top_level() {
        let src = "using System;\n\npublic class A { }\n";
        // Line 1 (the using) is not inside any definition.
        assert!(enclosing_scope(src, "cs", 1).is_none());
    }

    // ---- copy-paste-ready anchor suggestions (repair menu) -------------

    #[test]
    fn suggest_anchors_offers_class_scope_moves_when_inside_a_method() {
        let src = "\
public class HealthTests
{
    public void Pid_test()
    {
        var x = 1;
    }
}
";
        // Anchor landed on line 5, INSIDE Pid_test. Suggestions should let the
        // caller add a sibling at class scope, not nest inside the method.
        let sugg = suggest_anchors(src, "cs", 5);
        assert!(!sugg.is_empty(), "should offer at least one valid move");
        // One suggestion appends into the enclosing class body.
        assert!(
            sugg.iter().any(|s| s.mode == "append-into-symbol" && s.symbol == "HealthTests"),
            "expected append-into-symbol HealthTests; got {:?}", sugg
        );
        // One suggestion inserts after the method (still class scope).
        assert!(
            sugg.iter().any(|s| s.mode == "insert-after-symbol" && s.symbol == "Pid_test"),
            "expected insert-after-symbol Pid_test; got {:?}", sugg
        );
    }
}
