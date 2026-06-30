//! Products capability error (thiserror — library crate).

use thiserror::Error;
use sca_core::CoreError;

#[derive(Debug, Error)]
pub enum ProductsError {
    #[error("validation: {0}")]
    Validation(String),

    #[error("product not found: {0}")]
    NotFound(String),

    #[error(transparent)]
    Core(#[from] CoreError),
}

pub type ProductsResult<T> = Result<T, ProductsError>;
