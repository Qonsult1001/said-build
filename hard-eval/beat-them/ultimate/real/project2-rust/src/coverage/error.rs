//! Coverage capability error (thiserror — library crate).

use thiserror::Error;
use sca_core::CoreError;

#[derive(Debug, Error)]
pub enum CoverageError {
    #[error("validation: {0}")]
    Validation(String),

    #[error("coverage not found: {0}")]
    NotFound(String),

    #[error(transparent)]
    Core(#[from] CoreError),
}

pub type CoverageResult<T> = Result<T, CoverageError>;
