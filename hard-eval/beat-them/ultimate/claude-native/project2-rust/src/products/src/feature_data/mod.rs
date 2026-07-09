//! Feature-data use case.
//!
//! Canonical flow: validate → authorize (public read) → query → map to DTO →
//! return.

use crate::dto::FeatureDataDto;
use crate::error::ProductsResult;
use crate::ports::FeatureDataRepository;
use core::CoreError;
use std::sync::Arc;

pub struct FeatureDataService {
    repo: Arc<dyn FeatureDataRepository>,
}

impl FeatureDataService {
    pub fn new(repo: Arc<dyn FeatureDataRepository>) -> Self {
        Self { repo }
    }

    /// Fetch the feature data for a product sku.
    pub fn feature_data(&self, sku: &str) -> ProductsResult<FeatureDataDto> {
        // 1. validate
        if sku.trim().is_empty() {
            return Err(CoreError::Validation("sku required".into()).into());
        }

        // 2. authorize (public read — no gate)

        // 3. query
        let data = self.repo.by_sku(sku).ok_or_else(|| CoreError::NotFound {
            entity: "feature_data",
            id: sku.to_string(),
        })?;

        // 4/5. map to DTO + return
        Ok(FeatureDataDto::from(&data))
    }
}
