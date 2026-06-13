//! Single source of truth for language support in `.said`.
//!
//! Adding a new language: add ONE line to `register_languages()`. Nothing else
//! in the codebase changes. The CLI's `init` walker, the AST chunker, the
//! example tools, and `said list-languages` all derive from this registry.
//!
//! Supports 17 mainstream code languages + 5 markup/config formats + SQL.
//! Each language is statically linked at compile time. The full said.exe
//! stays a single binary; no runtime parser downloads.
//!
//! ## Why a wrapper
//! - Adding a language used to mean editing `match ext {...}` arms in 5
//!   different files. Now it's one line here.
//! - The `chunker` slot on each spec lets specialized formats (markdown,
//!   SQL) bypass the generic AST walk without us adding a new top-level
//!   match arm in `ast_chunk`.
//! - Replacing the implementation later (e.g. switching to
//!   `tree-sitter-language-pack`) is a change to this file alone.

#[cfg(feature = "code")]
use tree_sitter::Language;

use crate::code_search::CodeChunk;

/// One supported language.
#[cfg(feature = "code")]
pub struct LanguageSpec {
    /// Canonical name (lowercased, identifier-shaped). Mirrors the names
    /// used by tree-sitter-language-pack so we could swap implementations
    /// later with no API change.
    pub name: &'static str,
    /// File extensions (lowercased, no leading `.`) we treat as this language.
    pub extensions: &'static [&'static str],
    /// Tree-sitter grammar. Wrapped in `LangFn` so the registry can be
    /// constructed eagerly without `unsafe` static initialisation tricks.
    pub language: LangFn,
    /// Optional specialized chunker. When set, `ast_chunk` uses this
    /// instead of the generic AST walk. Used for:
    ///   - Markdown: header-based chunking (Cursor/Notion style)
    ///   - SQL/T-SQL: GO-batch splitting (tree-sitter-sequel exists and
    ///     said-forge uses it for wire-shape analysis, but it can't model
    ///     T-SQL `CREATE PROCEDURE` cleanly — chunking is happy with
    ///     GO-batches, so we keep that for the coarse-split use case).
    pub chunker: Option<fn(&str) -> Vec<CodeChunk>>,
}

/// Wrapper to avoid `Language: !Sync` issues across the static registry.
#[cfg(feature = "code")]
pub struct LangFn(pub fn() -> Language);

/// THE registry. Add a language by appending one entry here.
///
/// Order doesn't affect lookup correctness. Group by family for readability:
/// systems languages, scripting, JVM, mobile, markup, SQL.
#[cfg(feature = "code")]
pub fn register_languages() -> Vec<LanguageSpec> {
    vec![
        // ── Systems languages ────────────────────────────────────────
        LanguageSpec {
            name: "rust",
            extensions: &["rs"],
            language: LangFn(|| tree_sitter_rust::LANGUAGE.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "c",
            extensions: &["c", "h"],
            language: LangFn(|| tree_sitter_c::LANGUAGE.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "cpp",
            extensions: &["cpp", "cc", "cxx", "hpp", "hxx", "hh"],
            language: LangFn(|| tree_sitter_cpp::LANGUAGE.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "go",
            extensions: &["go"],
            language: LangFn(|| tree_sitter_go::LANGUAGE.into()),
            chunker: None,
        },

        // ── Scripting ────────────────────────────────────────────────
        LanguageSpec {
            name: "python",
            extensions: &["py"],
            language: LangFn(|| tree_sitter_python::LANGUAGE.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "javascript",
            extensions: &["js", "jsx", "mjs"],
            language: LangFn(|| tree_sitter_javascript::LANGUAGE.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "typescript",
            extensions: &["ts", "tsx"],
            language: LangFn(|| tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "ruby",
            extensions: &["rb"],
            language: LangFn(|| tree_sitter_ruby::LANGUAGE.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "php",
            extensions: &["php", "phtml"],
            language: LangFn(|| tree_sitter_php::LANGUAGE_PHP.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "lua",
            extensions: &["lua"],
            language: LangFn(|| tree_sitter_lua::LANGUAGE.into()),
            chunker: None,
        },

        // ── JVM family ───────────────────────────────────────────────
        LanguageSpec {
            name: "java",
            extensions: &["java"],
            language: LangFn(|| tree_sitter_java::LANGUAGE.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "kotlin",
            extensions: &["kt", "kts"],
            language: LangFn(|| tree_sitter_kotlin_ng::LANGUAGE.into()),
            chunker: None,
        },

        // ── .NET / Apple ─────────────────────────────────────────────
        LanguageSpec {
            name: "c_sharp",
            extensions: &["cs"],
            language: LangFn(|| tree_sitter_c_sharp::LANGUAGE.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "swift",
            extensions: &["swift"],
            language: LangFn(|| tree_sitter_swift::LANGUAGE.into()),
            chunker: None,
        },

        // ── Markup / config ──────────────────────────────────────────
        // Markdown uses header-based chunking, not AST walking — it's
        // the convergent design across Cursor / Notion / Copilot.
        LanguageSpec {
            name: "markdown",
            extensions: &["md", "markdown"],
            language: LangFn(|| tree_sitter_md::LANGUAGE.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "json",
            extensions: &["json"],
            language: LangFn(|| tree_sitter_json::LANGUAGE.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "yaml",
            extensions: &["yaml", "yml"],
            language: LangFn(|| tree_sitter_yaml::LANGUAGE.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "toml",
            extensions: &["toml"],
            language: LangFn(|| tree_sitter_toml_ng::LANGUAGE.into()),
            chunker: None,
        },

        // ── Documents / generic markup ───────────────────────────────
        LanguageSpec {
            name: "xml",
            extensions: &["xml", "xsd", "xsl", "xslt"],
            language: LangFn(|| tree_sitter_xml::LANGUAGE_XML.into()),
            chunker: None,
        },

        // ── Shell / scripting ────────────────────────────────────────
        // Bash also covers POSIX-shell-ish files (.zsh, .ksh) — they
        // parse OK under bash for our purposes.
        LanguageSpec {
            name: "bash",
            extensions: &["sh", "bash", "zsh", "ksh"],
            language: LangFn(|| tree_sitter_bash::LANGUAGE.into()),
            chunker: None,
        },

        // ── Infrastructure-as-code ───────────────────────────────────
        // HCL grammar parses Terraform `.tf` and `.tfvars` plus general
        // `.hcl` (e.g. Nomad jobs).
        LanguageSpec {
            name: "hcl",
            extensions: &["tf", "tfvars", "hcl"],
            language: LangFn(|| tree_sitter_hcl::LANGUAGE.into()),
            chunker: None,
        },

        // ── Operations / scripting ───────────────────────────────────
        LanguageSpec {
            name: "powershell",
            extensions: &["ps1", "psm1", "psd1"],
            language: LangFn(|| tree_sitter_powershell::LANGUAGE.into()),
            chunker: None,
        },
        LanguageSpec {
            name: "perl",
            extensions: &["pl", "pm", "t"],
            language: LangFn(|| tree_sitter_perl::LANGUAGE.into()),
            chunker: None,
        },
    ]
}

/// Look up a spec by file extension (case-insensitive). Returns None for
/// unsupported extensions; caller should fall back to whole-file storage.
///
/// SQL is intentionally NOT in `register_languages()` — sca-core's
/// code-chunking pipeline uses the GO-batch text parser for `sql`/`ddl`/
/// `tsql`. (said-forge has a tree-sitter-sequel dep for finer-grained
/// T-SQL wire-shape analysis in `proc_analysis.rs` — that's a different
/// concern, lives outside this language registry.)
#[cfg(feature = "code")]
pub fn lookup_by_extension(ext: &str) -> Option<LanguageSpec> {
    let ext_lc = ext.to_ascii_lowercase();
    register_languages()
        .into_iter()
        .find(|spec| spec.extensions.iter().any(|e| *e == ext_lc.as_str()))
}

/// All extensions handled by the AST chunker (NOT including SQL — that
/// uses its own dedicated path). Used by the CLI's file-walker filter.
#[cfg(feature = "code")]
pub fn supported_extensions() -> Vec<&'static str> {
    register_languages()
        .into_iter()
        .flat_map(|spec| spec.extensions.iter().copied().collect::<Vec<_>>())
        .collect()
}

/// Public list of supported language names (for `said list-languages` etc.).
#[cfg(feature = "code")]
pub fn supported_languages() -> Vec<&'static str> {
    register_languages().into_iter().map(|spec| spec.name).collect()
}

// =====================================================================
// Stub for non-`code` builds — keeps callers compiling with no work.
// =====================================================================

#[cfg(not(feature = "code"))]
pub fn lookup_by_extension(_ext: &str) -> Option<()> { None }

#[cfg(not(feature = "code"))]
pub fn supported_extensions() -> Vec<&'static str> { Vec::new() }

#[cfg(not(feature = "code"))]
pub fn supported_languages() -> Vec<&'static str> { Vec::new() }
