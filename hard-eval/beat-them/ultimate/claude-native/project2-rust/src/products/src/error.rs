//! Products typed error (thiserror).

use core::CoreError;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProductsError {
    #[error(transparent)]
    Core(#[from] CoreError),
}

pub type ProductsResult<T> = Result<T, ProductsError>;
