//! `said edit` core — re-exported from `sca-core` so the CLI and the MCP server
//! share one implementation. `.said` owns surgical edits; both front-ends call
//! the same pure functions (see `sca_core::edit`).

pub use sca_core::edit::*;
