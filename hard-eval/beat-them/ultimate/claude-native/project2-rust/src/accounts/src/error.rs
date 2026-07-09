//! Accounts typed error (thiserror).

use core::CoreError;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AccountsError {
    #[error("invalid credentials")]
    InvalidCredentials,

    #[error(transparent)]
    Core(#[from] CoreError),
}

pub type AccountsResult<T> = Result<T, AccountsError>;
