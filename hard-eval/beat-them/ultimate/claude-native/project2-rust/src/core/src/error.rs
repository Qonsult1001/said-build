//! Shared kernel error taxonomy.
//!
//! Library crates use `thiserror` (never `anyhow`). Every capability defines its
//! own `<Domain>Error` but composes these kernel variants for the cross-cutting
//! failure modes (validation, authorization, not-found) so surfaces can map them
//! to transport codes uniformly.

use thiserror::Error;

/// Cross-cutting failures shared by every capability.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    /// A request failed an invariant check before any side effect.
    #[error("validation failed: {0}")]
    Validation(String),

    /// The caller is not authenticated or lacks rights for the operation.
    #[error("not authorized: {0}")]
    Unauthorized(String),

    /// A required aggregate could not be located.
    #[error("{entity} not found: {id}")]
    NotFound { entity: &'static str, id: String },

    /// A persistence-layer port failed.
    #[error("storage failure: {0}")]
    Storage(String),
}

pub type CoreResult<T> = Result<T, CoreError>;
