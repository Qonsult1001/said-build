//! Proc framework — Rust port of the Python render+audit toolkit at
//! `dtcard/.forge/proc-framework/core/`.
//!
//! Reads the framework's data layer (TOML standards + profile + bundles +
//! SQL/C# fragments) and produces either:
//!   - rendered proc files for new endpoints
//!   - audit reports against deployed procs
//!
//! Design contract:
//!   - The data layer (standards/, profiles/, bundles/) is the single source
//!     of truth. This module READS it, never writes to it.
//!   - The `@said-managed: Fully|Ignore|Merge` marker convention is the same
//!     as the Python tool. A proc rendered by the Python tool and audited by
//!     this Rust tool must produce identical results.
//!   - This module is ADDITIVE to said-forge. It does not modify any other
//!     module. CLI subcommands (`said forge render`, `said forge audit`) are
//!     additive too.
//!
//! Why Rust (over the Python prototype):
//!   - Tree-sitter SQL parsing via `sca-core::code_search::ast_chunk` for
//!     AST-aware region boundary detection (catches malformed markers the
//!     Python regex auditor misses).
//!   - Optional LSP semantic queries via `sca-core::lsp_client` for the
//!     future C# generation work.
//!   - Single binary (`said.exe`) rather than Python + Rust mix.
//!   - Reuses `said-forge`'s existing `sql_catalog` for finding deployed
//!     procs by name rather than re-walking the filesystem.

pub mod manifest;
pub mod markers;
pub mod render;
pub mod shape;
pub mod audit;
pub mod cs_markers;
pub mod cs_audit;
pub mod cs_render;

pub use manifest::{load_profile, Bundle, EndpointRow, Profile, ProfileManifest, ProfilePaths};
pub use markers::{ManagedMode, Region};
pub use render::render_endpoint;
pub use shape::{Shape, ShapeRegion};
pub use audit::{audit_endpoint, AuditResult, AuditSummary};
pub use cs_markers::{parse_cs_regions, CsRegion};
pub use cs_audit::{audit_cs_endpoint, CsAuditResult, CsAuditSummary};
pub use cs_render::render_endpoint_cs;
