//! Search typed error (thiserror).

use core::CoreError;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SearchError {
    #[error("query too short: minimum {min} characters")]
    QueryTooShort { min: usize },

    #[error(transparent)]
    Core(#[from] CoreError),
}

pub type SearchResult<T> = Result<T, SearchError>;
