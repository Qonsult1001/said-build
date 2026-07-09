//! Search capability error (thiserror — library crate).

use thiserror::Error;
use sca_core::CoreError;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("validation: {0}")]
    Validation(String),

    #[error("lead not found: {0}")]
    NotFound(String),

    #[error(transparent)]
    Core(#[from] CoreError),
}

pub type SearchResult<T> = Result<T, SearchError>;
