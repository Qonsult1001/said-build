//! Dev Spec pipeline. See
//! `docs/superpowers/specs/2026-04-28-dev-spec-source-of-truth-design.md`.

pub mod borrow;
#[cfg(feature = "forge-sql-verify")]
pub mod drift;
pub mod erd;
pub mod openapi_emit;
pub mod parser;
pub mod registry_amend;
pub mod sql_emit;
pub mod types;

pub use types::{
    BorrowDecision, Column, DevSpecEndpoint, DevSpecParam, DevSpecSchema, Entity, Erd,
    ForeignKey,
};
