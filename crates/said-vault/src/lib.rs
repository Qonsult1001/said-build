//! said-vault — enterprise document vault on a .said file backend.
//!
//! See [`docs/superpowers/specs/2026-05-26-said-vault-track-b-design.md`]
//! for the design. v1 ships:
//!   - Centralized single-file vault per tenant (master/child reserved)
//!   - Pillar::Document frames for assets + manifests + roles
//!   - VAULT_TOMBSTONES section for byte-exact restore (enterprise tier)
//!   - Tag-based access control
//!   - BLAKE3 throughout (SQLite export re-hashes with SHA-256)

pub mod hasher;
pub mod manifest;
pub mod store;
pub mod ops;
pub mod access;
pub mod audit;
pub mod retention;
pub mod workstate;
pub mod parser;
pub mod rebuild;
pub mod preflate_recipe;
pub mod vault;

pub use vault::{SaidVault, CompareReport};
pub use manifest::Manifest;
pub use access::{Role, UserAssignment};
pub use workstate::WorkState;
