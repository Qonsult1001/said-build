//! The `Product` aggregate with embedded `FeatureData`.

use sca_core::ProductId;

use crate::error::{ProductsError, ProductsResult};

/// Marketing/feature metadata attached to a product (the `featuredata` read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureData {
    pub headline: String,
    pub bullets: Vec<String>,
    pub featured: bool,
}

/// A catalogue product. Private fields; built via `list`, which validates name
/// and price invariants in-aggregate.
#[derive(Debug, Clone)]
pub struct Product {
    id: ProductId,
    name: String,
    price_cents: u64,
    feature: FeatureData,
}

impl Product {
    pub fn list(
        id: ProductId,
        name: impl Into<String>,
        price_cents: u64,
        feature: FeatureData,
    ) -> ProductsResult<Self> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(ProductsError::Validation("empty product name".into()));
        }
        if price_cents == 0 {
            return Err(ProductsError::Validation("price must be > 0".into()));
        }
        if feature.headline.trim().is_empty() {
            return Err(ProductsError::Validation("empty feature headline".into()));
        }
        Ok(Self {
            id,
            name,
            price_cents,
            feature,
        })
    }

    pub fn id(&self) -> &ProductId {
        &self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn price_cents(&self) -> u64 {
        self.price_cents
    }
    pub fn feature(&self) -> &FeatureData {
        &self.feature
    }
    pub fn is_featured(&self) -> bool {
        self.feature.featured
    }
}
