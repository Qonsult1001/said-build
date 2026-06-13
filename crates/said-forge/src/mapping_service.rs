//! MappingService — the single correctness-critical module that decides
//! how a directive (OpenAPI / Dev Planning / XLSM) lines up with SQL
//! ground truth.
//!
//! Why one module? Before Phase 21 the logic was scattered across
//! `schema_diff`, `tech_grounding`, `gap_structured`, and `sql_catalog`
//! — each place had its own take on "is this column compatible", "is
//! this the right candidate table", "is this field mentioned by that
//! XLSM row". That meant three slightly-different answers to the same
//! question and no consistent way to attach confidence to the result.
//!
//! MappingService fixes that. It owns:
//! - catalog (SQL tables/procs/views/triggers)
//! - glossary (auto-built from `lookups.*` + FK graph)
//! - overrides (user-authored `.forge/mapping.toml`)
//! - a trace buffer — every decision gets a row in
//!   `.forge/mapping-trace.jsonl` so a developer can reconstruct WHY a
//!   given mapping won.
//!
//! Every public resolver returns `MappingResult<T>`: the value plus
//! the `Confidence` tier plus the rule id that produced it plus the
//! sources consulted. Downstream renderers (gap_structured, skill
//! regenerator, story writer) surface the confidence badge next to
//! each mapping so reviewers see at a glance where the soft spots
//! are.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;

use crate::directive::{Location, OpField, OpSpec};
use crate::schema::{Column, TableSchema};
use crate::schema_diff::Verdict;
use crate::sql_catalog::{SqlCatalog, SqlKind, SqlObject, TableOpKind};

pub mod glossary;
pub mod overrides;

pub use glossary::Glossary;
pub use overrides::{MappingOverrides, OverrideProc, OverrideTable};

// ─────────────────────────── confidence + result ───────────────────────────

/// Confidence tier for a mapping decision. Determines the badge the
/// renderer attaches and whether `--force` is needed to regenerate a
/// downstream artefact that depends on the mapping.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    /// No mapping produced. Caller must surface this in the gap report.
    None,
    /// Single-signal heuristic (case-insensitive name match, token
    /// overlap, singular-form match). Review before shipping.
    Low,
    /// Heuristic match survived multiple independent signals (e.g.
    /// Hungarian strip + type compatibility + FK alignment). Reviewable
    /// but usually correct.
    Medium,
    /// Exact symbol match (case-sensitive column name, exact proc
    /// signature, exact XLSM Req-ID). Treat as near-certain.
    High,
    /// User-authored override — ship it verbatim.
    Explicit,
}

impl Confidence {
    pub fn badge(self) -> &'static str {
        match self {
            Self::Explicit => "🔒 explicit",
            Self::High => "● high",
            Self::Medium => "◐ medium",
            Self::Low => "◯ low",
            Self::None => "✗ none",
        }
    }
}

/// Outcome of any MappingService resolver call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingResult<T> {
    pub value: T,
    pub confidence: Confidence,
    /// Rule id that produced this result. Stable string — used by the
    /// trace log and by tests.
    pub rule: &'static str,
    /// Human-readable sources consulted (table name, override path,
    /// glossary term, etc.) — one entry per source so renderers can
    /// list them verbatim.
    pub sources: Vec<String>,
}

impl<T> MappingResult<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> MappingResult<U> {
        MappingResult {
            value: f(self.value),
            confidence: self.confidence,
            rule: self.rule,
            sources: self.sources,
        }
    }
}

// ─────────────────────────── trace log ───────────────────────────

/// One line in `.forge/mapping-trace.jsonl`. Written on every resolver
/// call so a developer can replay what the service saw.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingTrace {
    /// Resolver name (`resolve_column`, `resolve_tables_for_op`, etc.).
    pub resolver: String,
    /// Input the resolver was called with, summarised to a single line.
    pub input: String,
    /// Result confidence tier as a short string.
    pub confidence: String,
    /// Rule id that fired.
    pub rule: String,
    /// Sources consulted.
    pub sources: Vec<String>,
}

// ─────────────────────────── typed references ───────────────────────────

/// A table reference with enough context for a renderer to print it
/// without re-reading the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRef {
    pub full_name: String,
    pub role: TableRole,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TableRole {
    Primary,
    Related,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnBinding {
    pub table: String,
    pub column: String,
    pub column_type: String,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcRef {
    pub full_name: String,
    /// Ops this proc performs on tables in scope. Empty when the
    /// override supplied the proc without body scanning.
    pub table_ops: Vec<(String, Vec<TableOpKind>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeCompat {
    pub compatible: bool,
    pub reason: Option<String>,
}

// ─────────────────────────── service ───────────────────────────

pub struct MappingService<'a> {
    pub catalog: &'a SqlCatalog,
    pub glossary: &'a Glossary,
    pub overrides: &'a MappingOverrides,
    trace: RefCell<Vec<MappingTrace>>,
}

impl<'a> MappingService<'a> {
    pub fn new(
        catalog: &'a SqlCatalog,
        glossary: &'a Glossary,
        overrides: &'a MappingOverrides,
    ) -> Self {
        Self {
            catalog,
            glossary,
            overrides,
            trace: RefCell::new(Vec::new()),
        }
    }

    /// Drain the trace buffer. Caller is expected to append to
    /// `.forge/mapping-trace.jsonl`.
    pub fn take_trace(&self) -> Vec<MappingTrace> {
        std::mem::take(&mut *self.trace.borrow_mut())
    }

    /// Drain the trace buffer and append every entry as one JSON
    /// object per line to `path`. Creates parent directories. Missing
    /// file is fine — we create it. Idempotent when called twice in a
    /// row (second call writes nothing because the buffer is drained).
    pub fn flush_trace_to(&self, path: &std::path::Path) -> crate::error::ForgeResult<usize> {
        let entries = self.take_trace();
        if entries.is_empty() {
            return Ok(0);
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| crate::error::ForgeError::Io {
                    path: parent.display().to_string(),
                    cause: e,
                })?;
            }
        }
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| crate::error::ForgeError::Io {
                path: path.display().to_string(),
                cause: e,
            })?;
        let count = entries.len();
        for entry in entries {
            let line = serde_json::to_string(&entry)?;
            writeln!(f, "{}", line).map_err(|e| crate::error::ForgeError::Io {
                path: path.display().to_string(),
                cause: e,
            })?;
        }
        Ok(count)
    }

    fn record(
        &self,
        resolver: &str,
        input: impl Into<String>,
        confidence: Confidence,
        rule: &'static str,
        sources: &[String],
    ) {
        self.trace.borrow_mut().push(MappingTrace {
            resolver: resolver.into(),
            input: input.into(),
            confidence: format!("{:?}", confidence),
            rule: rule.into(),
            sources: sources.to_vec(),
        });
    }

    // ─── resolver: tables for op ─────────────────────────────────────

    /// Resolve the tables an op touches. Override wins; falls back to
    /// token-scoring heuristic. Related tables are added via one-hop
    /// FK closure from the primary set.
    pub fn resolve_tables_for_op(&self, op: &OpSpec) -> Vec<MappingResult<TableRef>> {
        let input = op.slug.clone();

        if let Some(ovr) = self.overrides.for_op(&op.slug) {
            if !ovr.tables.is_empty() {
                let mut out = Vec::new();
                for t in &ovr.tables {
                    let sources = vec![format!("mapping.toml [{}] tables", op.slug)];
                    self.record(
                        "resolve_tables_for_op",
                        &input,
                        Confidence::Explicit,
                        "override.tables",
                        &sources,
                    );
                    out.push(MappingResult {
                        value: TableRef {
                            full_name: t.clone(),
                            role: TableRole::Primary,
                        },
                        confidence: Confidence::Explicit,
                        rule: "override.tables",
                        sources,
                    });
                }
                return self.extend_with_related(out);
            }
        }

        let tokens = op_tokens(op);
        let mut scored: Vec<(&TableSchema, i32)> = self
            .catalog
            .tables
            .iter()
            .map(|t| (t, score_table(t, &tokens)))
            .filter(|(_, s)| *s > 0)
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));

        if scored.is_empty() {
            self.record(
                "resolve_tables_for_op",
                &input,
                Confidence::None,
                "heuristic.no_tokens_matched",
                &[],
            );
            return Vec::new();
        }

        let top_score = scored[0].1;
        let mut out: Vec<MappingResult<TableRef>> = Vec::new();
        for (t, s) in scored.into_iter().take(4) {
            let conf = classify_score(s, top_score);
            let rule = if conf == Confidence::High {
                "heuristic.token_score_dominant"
            } else if conf == Confidence::Medium {
                "heuristic.token_score_strong"
            } else {
                "heuristic.token_score_weak"
            };
            let sources = vec![format!("token-score={}", s)];
            self.record("resolve_tables_for_op", &input, conf, rule, &sources);
            out.push(MappingResult {
                value: TableRef {
                    full_name: t.full_name(),
                    role: TableRole::Primary,
                },
                confidence: conf,
                rule,
                sources,
            });
        }
        self.extend_with_related(out)
    }

    fn extend_with_related(
        &self,
        mut primary: Vec<MappingResult<TableRef>>,
    ) -> Vec<MappingResult<TableRef>> {
        let mut seen: std::collections::BTreeSet<String> =
            primary.iter().map(|p| p.value.full_name.clone()).collect();
        let primary_names: Vec<String> = seen.iter().cloned().collect();
        for name in &primary_names {
            if let Some(t) = self.catalog.find_table(name) {
                for fk in &t.foreign_keys {
                    let target_full = match &fk.ref_schema {
                        Some(s) => format!("{}.{}", s, fk.ref_table),
                        None => fk.ref_table.clone(),
                    };
                    if seen.contains(&target_full) {
                        continue;
                    }
                    if self.catalog.find_table(&target_full).is_some() {
                        seen.insert(target_full.clone());
                        let sources = vec![format!("FK from {} via {}", name, fk.column)];
                        self.record(
                            "resolve_tables_for_op",
                            name,
                            Confidence::High,
                            "heuristic.fk_one_hop",
                            &sources,
                        );
                        primary.push(MappingResult {
                            value: TableRef {
                                full_name: target_full,
                                role: TableRole::Related,
                            },
                            confidence: Confidence::High,
                            rule: "heuristic.fk_one_hop",
                            sources,
                        });
                    }
                }
            }
        }
        primary
    }

    // ─── resolver: column binding ────────────────────────────────────

    /// Resolve a field to a column on one of the supplied tables.
    /// Override > glossary hint > exact name > case-insensitive >
    /// Hungarian strip > normalised equality.
    pub fn resolve_column(
        &self,
        op_slug: &str,
        field: &OpField,
        tables: &[&TableSchema],
    ) -> MappingResult<ColumnBinding> {
        let input = format!("{} / {}", op_slug, field.name);

        if !matches!(field.location, Location::Body | Location::Path) {
            self.record(
                "resolve_column",
                &input,
                Confidence::None,
                "skipped.not_body_or_path",
                &[],
            );
            return MappingResult {
                value: ColumnBinding {
                    table: String::new(),
                    column: String::new(),
                    column_type: String::new(),
                    verdict: Verdict::Skipped,
                },
                confidence: Confidence::None,
                rule: "skipped.not_body_or_path",
                sources: Vec::new(),
            };
        }

        if let Some(ovr) = self.overrides.for_op(op_slug) {
            if let Some(col) = ovr.columns.get(&field.name) {
                let sources = vec![format!("mapping.toml [{}] columns", op_slug)];
                self.record(
                    "resolve_column",
                    &input,
                    Confidence::Explicit,
                    "override.columns",
                    &sources,
                );
                let (table_name, column_name) = split_table_column(col);
                let verdict = self.build_verdict_from_override(field, &table_name, &column_name);
                return MappingResult {
                    value: ColumnBinding {
                        table: table_name,
                        column: column_name,
                        column_type: String::new(),
                        verdict,
                    },
                    confidence: Confidence::Explicit,
                    rule: "override.columns",
                    sources,
                };
            }
        }

        let field_norm = normalise_name(&field.name);
        let mut sources: Vec<String> = Vec::new();

        for table in tables {
            if let Some(col) = table.columns.iter().find(|c| c.name == field.name) {
                sources.push(format!("exact match on {}", table.full_name()));
                let verdict = compare_field_to_column(field, col, table);
                let conf = confidence_from_verdict(&verdict, Confidence::High);
                self.record(
                    "resolve_column",
                    &input,
                    conf,
                    "heuristic.exact_name",
                    &sources,
                );
                return MappingResult {
                    value: ColumnBinding {
                        table: table.full_name(),
                        column: col.name.clone(),
                        column_type: format_col_type(col),
                        verdict,
                    },
                    confidence: conf,
                    rule: "heuristic.exact_name",
                    sources,
                };
            }
        }
        for table in tables {
            if let Some(col) = table
                .columns
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(&field.name))
            {
                sources.push(format!("case-insensitive on {}", table.full_name()));
                let verdict = compare_field_to_column(field, col, table);
                let conf = confidence_from_verdict(&verdict, Confidence::High);
                self.record(
                    "resolve_column",
                    &input,
                    conf,
                    "heuristic.case_insensitive",
                    &sources,
                );
                return MappingResult {
                    value: ColumnBinding {
                        table: table.full_name(),
                        column: col.name.clone(),
                        column_type: format_col_type(col),
                        verdict,
                    },
                    confidence: conf,
                    rule: "heuristic.case_insensitive",
                    sources,
                };
            }
        }
        for table in tables {
            if let Some(col) = table.columns.iter().find(|c| {
                let stripped = strip_prefix(&c.name);
                normalise_name(&stripped) == field_norm
            }) {
                sources.push(format!("hungarian strip on {}", table.full_name()));
                let verdict = compare_field_to_column(field, col, table);
                let conf = confidence_from_verdict(&verdict, Confidence::Medium);
                self.record(
                    "resolve_column",
                    &input,
                    conf,
                    "heuristic.hungarian_strip",
                    &sources,
                );
                return MappingResult {
                    value: ColumnBinding {
                        table: table.full_name(),
                        column: col.name.clone(),
                        column_type: format_col_type(col),
                        verdict,
                    },
                    confidence: conf,
                    rule: "heuristic.hungarian_strip",
                    sources,
                };
            }
        }
        for table in tables {
            if let Some(col) = table
                .columns
                .iter()
                .find(|c| normalise_name(&c.name) == field_norm)
            {
                sources.push(format!("normalised on {}", table.full_name()));
                let verdict = compare_field_to_column(field, col, table);
                let conf = confidence_from_verdict(&verdict, Confidence::Low);
                self.record(
                    "resolve_column",
                    &input,
                    conf,
                    "heuristic.normalised_equality",
                    &sources,
                );
                return MappingResult {
                    value: ColumnBinding {
                        table: table.full_name(),
                        column: col.name.clone(),
                        column_type: format_col_type(col),
                        verdict,
                    },
                    confidence: conf,
                    rule: "heuristic.normalised_equality",
                    sources,
                };
            }
        }

        self.record(
            "resolve_column",
            &input,
            Confidence::None,
            "missing.no_candidate",
            &sources,
        );
        MappingResult {
            value: ColumnBinding {
                table: String::new(),
                column: String::new(),
                column_type: String::new(),
                verdict: Verdict::Missing,
            },
            confidence: Confidence::None,
            rule: "missing.no_candidate",
            sources,
        }
    }

    fn build_verdict_from_override(
        &self,
        field: &OpField,
        table_name: &str,
        column_name: &str,
    ) -> Verdict {
        if let Some(t) = self.catalog.find_table(table_name) {
            if let Some(col) = t.columns.iter().find(|c| c.name == column_name) {
                return compare_field_to_column(field, col, t);
            }
        }
        Verdict::Matched {
            table: table_name.to_string(),
            column: column_name.to_string(),
            column_type: String::new(),
        }
    }

    // ─── resolver: proc for op ───────────────────────────────────────

    /// Resolve the stored procedure(s) an op should call. Override
    /// wins; otherwise pick procs from the catalog that reference any
    /// table in scope. Ops that write (INSERT/UPDATE/DELETE) get
    /// higher confidence for POST/PUT/DELETE; SELECT-only procs get
    /// higher confidence for GET.
    pub fn resolve_proc_for_op(
        &self,
        op: &OpSpec,
        tables: &[String],
    ) -> Vec<MappingResult<ProcRef>> {
        let input = op.slug.clone();

        if let Some(ovr) = self.overrides.for_op(&op.slug) {
            if !ovr.procs.is_empty() {
                let mut out = Vec::new();
                for p in &ovr.procs {
                    let sources = vec![format!("mapping.toml [{}] procs", op.slug)];
                    self.record(
                        "resolve_proc_for_op",
                        &input,
                        Confidence::Explicit,
                        "override.procs",
                        &sources,
                    );
                    let table_ops = match self
                        .catalog
                        .objects
                        .iter()
                        .find(|o| o.kind == SqlKind::Procedure && o.full_name() == p.name)
                    {
                        Some(sql_obj) => sql_obj
                            .ops
                            .iter()
                            .map(|to| (to.table.clone(), to.kinds.clone()))
                            .collect(),
                        None => Vec::new(),
                    };
                    out.push(MappingResult {
                        value: ProcRef {
                            full_name: p.name.clone(),
                            table_ops,
                        },
                        confidence: Confidence::Explicit,
                        rule: "override.procs",
                        sources,
                    });
                }
                return out;
            }
        }

        let method = op.method.as_deref().unwrap_or("").to_ascii_uppercase();
        let wants_write = matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE");

        let mut out = Vec::new();
        let referencing: Vec<&SqlObject> = self
            .catalog
            .objects_referencing(tables)
            .into_iter()
            .filter(|o| o.kind == SqlKind::Procedure)
            .collect();

        for obj in referencing {
            let kinds_present: Vec<TableOpKind> = obj
                .ops
                .iter()
                .flat_map(|to| to.kinds.iter().copied())
                .collect();
            let has_write = kinds_present.iter().any(|k| {
                matches!(
                    k,
                    TableOpKind::Insert | TableOpKind::Update | TableOpKind::Delete
                )
            });
            let aligned = (wants_write && has_write) || (!wants_write && !kinds_present.is_empty());
            let conf = if aligned {
                Confidence::Medium
            } else {
                Confidence::Low
            };
            let rule = if aligned {
                "heuristic.proc_method_aligned"
            } else {
                "heuristic.proc_method_misaligned"
            };
            let sources = vec![format!(
                "catalog proc {} references {}",
                obj.full_name(),
                obj.referenced_tables.join(", ")
            )];
            self.record("resolve_proc_for_op", &input, conf, rule, &sources);
            out.push(MappingResult {
                value: ProcRef {
                    full_name: obj.full_name(),
                    table_ops: obj
                        .ops
                        .iter()
                        .map(|to| (to.table.clone(), to.kinds.clone()))
                        .collect(),
                },
                confidence: conf,
                rule,
                sources,
            });
        }

        if out.is_empty() {
            self.record(
                "resolve_proc_for_op",
                &input,
                Confidence::None,
                "missing.no_proc_references",
                &[],
            );
        }
        out
    }

    // ─── resolver: type compatibility ────────────────────────────────

    /// Expose the type-compat check through the service so renderers
    /// and tests share one source of truth. Glossary entries can widen
    /// compat when a logical type is a documented enum stored as a
    /// code column (e.g. `country_code` ↔ VARCHAR(2)).
    pub fn resolve_type_compatibility(
        &self,
        logical: &str,
        format: Option<&str>,
        sql_type: &str,
    ) -> MappingResult<TypeCompat> {
        let input = format!("{} / {} → {}", logical, format.unwrap_or("-"), sql_type);

        if let Some(term) = self.glossary.term(logical) {
            if term.sql_types.iter().any(|t| t.eq_ignore_ascii_case(sql_type)) {
                let sources = vec![format!("glossary term `{}`", logical)];
                self.record(
                    "resolve_type_compatibility",
                    &input,
                    Confidence::High,
                    "glossary.widened_compat",
                    &sources,
                );
                return MappingResult {
                    value: TypeCompat {
                        compatible: true,
                        reason: None,
                    },
                    confidence: Confidence::High,
                    rule: "glossary.widened_compat",
                    sources,
                };
            }
        }

        let compat = type_compatible_core(logical, format, sql_type);
        let conf = if compat.compatible {
            Confidence::High
        } else {
            Confidence::None
        };
        let rule = if compat.compatible {
            "compat.matrix_match"
        } else {
            "compat.matrix_reject"
        };
        self.record("resolve_type_compatibility", &input, conf, rule, &[]);
        MappingResult {
            value: compat,
            confidence: conf,
            rule,
            sources: Vec::new(),
        }
    }
}

// ─────────────────────────── helpers moved from schema_diff ───────────────────────────
// These are `pub(crate)` so schema_diff can keep delegating to the
// service without re-deriving them. Once every call site routes
// through MappingService the schema_diff copies become thin wrappers.

pub(crate) fn op_tokens(op: &OpSpec) -> Vec<String> {
    let mut src = String::new();
    if let Some(p) = &op.path {
        src.push_str(p);
    }
    src.push(' ');
    src.push_str(&op.label);
    src.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .filter(|t| t.len() > 2)
        .collect()
}

pub(crate) fn score_table(t: &TableSchema, tokens: &[String]) -> i32 {
    let mut score = 0i32;
    let name_lc = t.name.to_ascii_lowercase();
    let schema_lc = t.schema.as_deref().unwrap_or("").to_ascii_lowercase();
    for tok in tokens {
        let tok_singular = singular(tok);
        if schema_lc.contains(&tok_singular) {
            score += 3;
        }
        if name_lc.contains(&tok_singular) {
            score += 2;
        }
        if name_lc.contains(tok) {
            score += 1;
        }
    }
    score
}

pub(crate) fn singular(s: &str) -> String {
    if s.ends_with("ies") {
        let mut out = s.to_string();
        out.truncate(out.len() - 3);
        out.push('y');
        return out;
    }
    if s.ends_with('s') && !s.ends_with("ss") {
        return s[..s.len() - 1].to_string();
    }
    s.to_string()
}

pub(crate) fn normalise_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

pub(crate) fn strip_prefix(col_name: &str) -> String {
    let mut parts = col_name.splitn(2, '_');
    let first = parts.next().unwrap_or("");
    let rest = parts.next();
    if rest.is_some()
        && (2..=4).contains(&first.len())
        && first.chars().all(|c| c.is_ascii_lowercase())
    {
        rest.unwrap().to_string()
    } else {
        col_name.to_string()
    }
}

pub(crate) fn format_col_type(col: &Column) -> String {
    match (col.length, col.precision) {
        (Some(-1), _) => format!("{}(MAX)", col.sql_type),
        (Some(n), _) => format!("{}({})", col.sql_type, n),
        (_, Some((p, s))) => format!("{}({}, {})", col.sql_type, p, s),
        _ => col.sql_type.clone(),
    }
}

pub(crate) fn compare_field_to_column(
    field: &OpField,
    col: &Column,
    table: &TableSchema,
) -> Verdict {
    let compat = type_compatible_core(&field.logical_type, field.format.as_deref(), &col.sql_type);
    if !compat.compatible {
        return Verdict::TypeMismatch {
            table: table.full_name(),
            column: col.name.clone(),
            column_type: col.sql_type.clone(),
            reason: compat
                .reason
                .unwrap_or_else(|| "incompatible type".into()),
        };
    }
    if let (Some(api_max), Some(col_len)) = (field.max_length, col.length) {
        if col_len != -1 && (api_max as i32) > col_len {
            return Verdict::LengthMismatch {
                table: table.full_name(),
                column: col.name.clone(),
                column_length: col_len,
                api_max_length: api_max,
            };
        }
    }
    if field.required != !col.nullable && !col.has_default && !col.identity {
        return Verdict::NullabilityMismatch {
            table: table.full_name(),
            column: col.name.clone(),
            column_nullable: col.nullable,
            api_required: field.required,
        };
    }
    Verdict::Matched {
        table: table.full_name(),
        column: col.name.clone(),
        column_type: format_col_type(col),
    }
}

pub(crate) fn type_compatible_core(logical: &str, format: Option<&str>, sql: &str) -> TypeCompat {
    let logical = logical.to_ascii_lowercase();
    let sql_upper = sql.to_ascii_uppercase();

    let string_types = ["VARCHAR", "NVARCHAR", "CHAR", "NCHAR", "TEXT", "NTEXT", "XML", "SYSNAME"];
    let int_types = ["INT", "BIGINT", "SMALLINT", "TINYINT"];
    let numeric_types = ["DECIMAL", "NUMERIC", "FLOAT", "REAL", "MONEY", "SMALLMONEY"];
    let bool_types = ["BIT"];
    let uuid_types = ["UNIQUEIDENTIFIER"];
    let datetime_types = [
        "DATETIME",
        "DATETIMEOFFSET",
        "DATETIME2",
        "SMALLDATETIME",
        "DATE",
        "TIME",
    ];

    let is = |list: &[&str]| list.iter().any(|t| &sql_upper == *t);

    match (logical.as_str(), format) {
        ("string", Some(fmt)) if fmt.eq_ignore_ascii_case("uuid") => TypeCompat {
            compatible: is(&uuid_types),
            reason: if is(&uuid_types) {
                None
            } else {
                Some(format!(
                    "API format=uuid but SQL column is {} (expected UNIQUEIDENTIFIER)",
                    sql_upper
                ))
            },
        },
        ("string", Some(fmt))
            if fmt.eq_ignore_ascii_case("date")
                || fmt.eq_ignore_ascii_case("date-time")
                || fmt.eq_ignore_ascii_case("time") =>
        {
            TypeCompat {
                compatible: is(&datetime_types) || is(&string_types),
                reason: if is(&datetime_types) || is(&string_types) {
                    None
                } else {
                    Some(format!(
                        "API format={} but SQL column is {} (expected DATE/DATETIME or VARCHAR)",
                        fmt, sql_upper
                    ))
                },
            }
        }
        ("string", _) => TypeCompat {
            compatible: is(&string_types) || is(&uuid_types) || is(&datetime_types),
            reason: if is(&string_types) || is(&uuid_types) || is(&datetime_types) {
                None
            } else {
                Some(format!(
                    "API type=string but SQL column is {}",
                    sql_upper
                ))
            },
        },
        ("integer", _) => TypeCompat {
            compatible: is(&int_types) || is(&numeric_types),
            reason: if is(&int_types) || is(&numeric_types) {
                None
            } else {
                Some(format!("API type=integer but SQL column is {}", sql_upper))
            },
        },
        ("number", _) => TypeCompat {
            compatible: is(&numeric_types) || is(&int_types),
            reason: if is(&numeric_types) || is(&int_types) {
                None
            } else {
                Some(format!("API type=number but SQL column is {}", sql_upper))
            },
        },
        ("boolean", _) => TypeCompat {
            compatible: is(&bool_types) || is(&int_types),
            reason: if is(&bool_types) || is(&int_types) {
                None
            } else {
                Some(format!("API type=boolean but SQL column is {}", sql_upper))
            },
        },
        ("array", _) | ("object", _) => TypeCompat {
            compatible: sql_upper == "NVARCHAR" || sql_upper == "VARCHAR" || sql_upper == "XML",
            reason: if sql_upper == "NVARCHAR" || sql_upper == "VARCHAR" || sql_upper == "XML" {
                None
            } else {
                Some(format!(
                    "API type={} requires serialised storage; SQL column is {} (expected NVARCHAR / XML)",
                    logical, sql_upper
                ))
            },
        },
        _ => TypeCompat {
            compatible: true,
            reason: None,
        },
    }
}

fn classify_score(score: i32, top: i32) -> Confidence {
    if score == 0 {
        Confidence::None
    } else if score >= 5 || (top > 0 && score == top && score >= 3) {
        Confidence::High
    } else if score >= 2 {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}

fn confidence_from_verdict(v: &Verdict, base: Confidence) -> Confidence {
    match v {
        Verdict::Matched { .. } => base,
        Verdict::TypeMismatch { .. }
        | Verdict::LengthMismatch { .. }
        | Verdict::NullabilityMismatch { .. } => match base {
            Confidence::High => Confidence::Medium,
            Confidence::Medium => Confidence::Low,
            other => other,
        },
        Verdict::Missing | Verdict::Skipped => Confidence::None,
    }
}

fn split_table_column(combined: &str) -> (String, String) {
    if let Some(idx) = combined.rfind('.') {
        let (t, c) = combined.split_at(idx);
        (t.to_string(), c[1..].to_string())
    } else {
        (String::new(), combined.to_string())
    }
}

// ─────────────────────────── tests ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::{Location, OpField, OpSpec};
    use crate::schema::parse_create_table;
    use crate::sql_catalog::{SqlKind, SqlObject, TableOp, TableOpKind};

    fn sample_catalog() -> SqlCatalog {
        let table = parse_create_table(
            r#"CREATE TABLE [cardholder].[cpf_Client_Profile] (
                [cpf_Profile_Id]       UNIQUEIDENTIFIER   NOT NULL,
                [cpf_Easy_Profile_Id]  VARCHAR (60)       NOT NULL,
                [cpf_Last_Name]        NVARCHAR (1024)    NULL,
                [ptl_Code]             VARCHAR (10)       NOT NULL,
                CONSTRAINT [PK_cpf] PRIMARY KEY ([cpf_Profile_Id]),
                CONSTRAINT [FK_ptl] FOREIGN KEY ([ptl_Code]) REFERENCES [lookups].[ptl_Passport] ([ptl_Code])
            );"#,
        )
        .unwrap();
        let lookup = parse_create_table(
            r#"CREATE TABLE [lookups].[ptl_Passport] (
                [ptl_Code]        VARCHAR (10) NOT NULL,
                [ptl_Description] NVARCHAR (255) NULL,
                CONSTRAINT [PK_ptl] PRIMARY KEY ([ptl_Code])
            );"#,
        )
        .unwrap();
        let proc_insert = SqlObject {
            kind: SqlKind::Procedure,
            schema: Some("cardholder".into()),
            name: "p_create_cardholder".into(),
            referenced_tables: vec!["cardholder.cpf_Client_Profile".into()],
            ops: vec![TableOp {
                table: "cardholder.cpf_Client_Profile".into(),
                kinds: vec![TableOpKind::Insert],
            }],
            source_path: "proc_create.sql".into(),
        };
        let proc_get = SqlObject {
            kind: SqlKind::Procedure,
            schema: Some("cardholder".into()),
            name: "p_get_cardholder".into(),
            referenced_tables: vec!["cardholder.cpf_Client_Profile".into()],
            ops: vec![TableOp {
                table: "cardholder.cpf_Client_Profile".into(),
                kinds: vec![TableOpKind::Select],
            }],
            source_path: "proc_get.sql".into(),
        };
        SqlCatalog {
            tables: vec![table, lookup],
            objects: vec![proc_insert, proc_get],
        }
    }

    fn op_create_cardholder() -> OpSpec {
        OpSpec {
            slug: "post-cardholders".into(),
            label: "POST /cardholders".into(),
            method: Some("POST".into()),
            path: Some("/cardholders".into()),
            summary: None,
            fields: vec![OpField {
                name: "last_name".into(),
                logical_type: "string".into(),
                location: Location::Body,
                required: true,
                max_length: Some(1024),
                format: None,
                description: None,
            }],
            adapter: "openapi".into(),
            source: "test".into(),
        }
    }

    #[test]
    fn resolves_tables_via_token_score_and_one_hop_fk() {
        let cat = sample_catalog();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&cat, &g, &o);
        let tables = svc.resolve_tables_for_op(&op_create_cardholder());
        assert!(tables.iter().any(|t| t.value.full_name == "cardholder.cpf_Client_Profile"
            && t.value.role == TableRole::Primary));
        assert!(tables.iter().any(|t| t.value.full_name == "lookups.ptl_Passport"
            && t.value.role == TableRole::Related));
    }

    #[test]
    fn override_tables_win_over_heuristic() {
        let cat = sample_catalog();
        let g = Glossary::default();
        let mut o = MappingOverrides::default();
        o.insert_op_table("post-cardholders", "cardholder.cpf_Client_Profile".into());
        let svc = MappingService::new(&cat, &g, &o);
        let tables = svc.resolve_tables_for_op(&op_create_cardholder());
        let explicit = tables.iter().find(|t| t.confidence == Confidence::Explicit);
        assert!(explicit.is_some());
        assert_eq!(explicit.unwrap().rule, "override.tables");
    }

    #[test]
    fn resolve_column_uses_hungarian_strip_with_medium_confidence() {
        let cat = sample_catalog();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&cat, &g, &o);
        let t = cat.find_table("cardholder.cpf_Client_Profile").unwrap();
        let field = OpField {
            name: "last_name".into(),
            logical_type: "string".into(),
            location: Location::Body,
            required: false,
            max_length: Some(1024),
            format: None,
            description: None,
        };
        let res = svc.resolve_column("post-cardholders", &field, &[t]);
        assert_eq!(res.rule, "heuristic.hungarian_strip");
        assert_eq!(res.confidence, Confidence::Medium);
        assert!(matches!(res.value.verdict, Verdict::Matched { .. }));
    }

    #[test]
    fn missing_column_records_none_confidence() {
        let cat = sample_catalog();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&cat, &g, &o);
        let t = cat.find_table("cardholder.cpf_Client_Profile").unwrap();
        let field = OpField {
            name: "nationality".into(),
            logical_type: "string".into(),
            location: Location::Body,
            required: true,
            max_length: None,
            format: None,
            description: None,
        };
        let res = svc.resolve_column("post-cardholders", &field, &[t]);
        assert_eq!(res.confidence, Confidence::None);
        assert!(matches!(res.value.verdict, Verdict::Missing));
    }

    #[test]
    fn proc_picker_prefers_write_proc_for_post_method() {
        let cat = sample_catalog();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&cat, &g, &o);
        let procs = svc.resolve_proc_for_op(
            &op_create_cardholder(),
            &["cardholder.cpf_Client_Profile".into()],
        );
        let create = procs
            .iter()
            .find(|p| p.value.full_name == "cardholder.p_create_cardholder")
            .unwrap();
        let get = procs
            .iter()
            .find(|p| p.value.full_name == "cardholder.p_get_cardholder")
            .unwrap();
        assert!(create.confidence >= get.confidence);
    }

    #[test]
    fn override_proc_beats_heuristic_with_explicit_confidence() {
        let cat = sample_catalog();
        let g = Glossary::default();
        let mut o = MappingOverrides::default();
        o.insert_op_proc("post-cardholders", "cardholder.p_txn_Update_Cardholder".into());
        let svc = MappingService::new(&cat, &g, &o);
        let procs = svc.resolve_proc_for_op(
            &op_create_cardholder(),
            &["cardholder.cpf_Client_Profile".into()],
        );
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].confidence, Confidence::Explicit);
        assert_eq!(procs[0].value.full_name, "cardholder.p_txn_Update_Cardholder");
    }

    #[test]
    fn glossary_widens_type_compat_for_country_code() {
        let cat = sample_catalog();
        let mut g = Glossary::default();
        g.insert_term(glossary::Term {
            name: "country_code".into(),
            description: "ISO-3166 alpha-2".into(),
            sql_types: vec!["VARCHAR".into(), "CHAR".into()],
            sources: vec!["lookups.col_Country_Lookup".into()],
        });
        let o = MappingOverrides::default();
        let svc = MappingService::new(&cat, &g, &o);
        let res = svc.resolve_type_compatibility("country_code", None, "VARCHAR");
        assert!(res.value.compatible);
        assert_eq!(res.rule, "glossary.widened_compat");
    }

    #[test]
    fn trace_records_every_decision() {
        let cat = sample_catalog();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&cat, &g, &o);
        let _ = svc.resolve_tables_for_op(&op_create_cardholder());
        let trace = svc.take_trace();
        assert!(!trace.is_empty());
        assert!(trace.iter().any(|t| t.resolver == "resolve_tables_for_op"));
    }

    #[test]
    fn flush_trace_writes_jsonl_and_drains_buffer() {
        let cat = sample_catalog();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&cat, &g, &o);
        let _ = svc.resolve_tables_for_op(&op_create_cardholder());
        let dir = tempfile::tempdir().unwrap();
        let trace_path = dir.path().join("sub").join("mapping-trace.jsonl");
        let written = svc.flush_trace_to(&trace_path).unwrap();
        assert!(written > 0);
        let contents = std::fs::read_to_string(&trace_path).unwrap();
        assert!(contents.contains("resolve_tables_for_op"));
        // Each line must be independently parseable as JSON.
        for line in contents.lines() {
            let _: MappingTrace = serde_json::from_str(line).unwrap();
        }
        // Second flush → buffer drained, file unchanged in row count.
        let second = svc.flush_trace_to(&trace_path).unwrap();
        assert_eq!(second, 0);
    }

    #[test]
    fn take_trace_drains_buffer() {
        let cat = sample_catalog();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&cat, &g, &o);
        let _ = svc.resolve_tables_for_op(&op_create_cardholder());
        let first = svc.take_trace();
        assert!(!first.is_empty());
        let second = svc.take_trace();
        assert!(second.is_empty());
    }
}
