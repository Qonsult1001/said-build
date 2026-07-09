//! Coverage typed error (thiserror).

use core::CoreError;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoverageError {
    #[error(transparent)]
    Core(#[from] CoreError),
}

pub type CoverageResult<T> = Result<T, CoverageError>;
