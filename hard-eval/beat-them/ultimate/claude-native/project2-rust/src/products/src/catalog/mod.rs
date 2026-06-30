//! Product (catalogue) use case.
//!
//! Canonical flow: validate → authorize (public catalogue read) → query → map to
//! DTO → return.

use crate::dto::ProductDto;
use crate::error::ProductsResult;
use crate::ports::ProductRepository;
use core::CoreError;
use std::sync::Arc;

pub struct ProductService {
    repo: Arc<dyn ProductRepository>,
}

impl ProductService {
    pub fn new(repo: Arc<dyn ProductRepository>) -> Self {
        Self { repo }
    }

    /// Fetch a single product by sku.
    pub fn product(&self, sku: &str) -> ProductsResult<ProductDto> {
        // 1. validate
        if sku.trim().is_empty() {
            return Err(CoreError::Validation("sku required".into()).into());
        }

        // 2. authorize (public catalogue — no gate)

        // 3. query
        let product = self.repo.by_sku(sku).ok_or_else(|| CoreError::NotFound {
            entity: "product",
            id: sku.to_string(),
        })?;

        // 4/5. map to DTO + return
        Ok(ProductDto::from(&product))
    }

    /// List the whole catalogue.
    pub fn catalog(&self) -> ProductsResult<Vec<ProductDto>> {
        Ok(self.repo.all().iter().map(ProductDto::from).collect())
    }
}
