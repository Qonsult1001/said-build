//! Accounts capability error (thiserror — this is a library crate).

use thiserror::Error;
use sca_core::CoreError;

#[derive(Debug, Error)]
pub enum AccountsError {
    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("validation: {0}")]
    Validation(String),

    #[error("account not found: {0}")]
    NotFound(String),

    #[error(transparent)]
    Core(#[from] CoreError),
}

pub type AccountsResult<T> = Result<T, AccountsError>;
