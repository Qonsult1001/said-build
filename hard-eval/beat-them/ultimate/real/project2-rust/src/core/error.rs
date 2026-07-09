//! Shared kernel error type.
//!
//! Per the architecture-rust constitution: library/capability/core crates use
//! `thiserror`, never `anyhow`, and never `Result<T, String>` across a public
//! boundary. This is the one typed enum the kernel owns; capability crates wrap
//! it or define their own `<Domain>Error` and convert inward.

use thiserror::Error;

/// A classification a caller can branch on (retry, reject, escalate) without
/// string-matching the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    /// The request was malformed or violated an invariant — never retry.
    Validation,
    /// The requested aggregate does not exist.
    NotFound,
    /// The caller is not permitted to perform the action.
    Unauthorized,
    /// A downstream port failed transiently — safe to retry.
    Transient,
}

/// The kernel-wide error. Capability crates may define their own typed error
/// and convert into/out of this at the port boundary.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("validation failed: {0}")]
    Validation(String),

    #[error("{entity} not found: {id}")]
    NotFound { entity: &'static str, id: String },

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("repository failure: {0}")]
    Repository(String),
}

impl CoreError {
    /// Map an error to its retry/reject classification.
    pub fn class(&self) -> FailureClass {
        match self {
            CoreError::Validation(_) => FailureClass::Validation,
            CoreError::NotFound { .. } => FailureClass::NotFound,
            CoreError::Unauthorized(_) => FailureClass::Unauthorized,
            CoreError::Repository(_) => FailureClass::Transient,
        }
    }
}

/// Crate-wide result alias (`<Domain>Result<T>` convention).
pub type CoreResult<T> = Result<T, CoreError>;
