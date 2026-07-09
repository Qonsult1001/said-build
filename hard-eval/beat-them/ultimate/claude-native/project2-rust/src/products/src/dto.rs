//! Products DTOs — projections crossing the surface boundary.

use crate::model::{FeatureData, Product};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductDto {
    pub id: String,
    pub sku: String,
    pub name: String,
    pub price_cents: u64,
}

impl From<&Product> for ProductDto {
    fn from(p: &Product) -> Self {
        Self {
            id: p.id().to_string(),
            sku: p.sku().to_string(),
            name: p.name().to_string(),
            price_cents: p.price_cents(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureDataDto {
    pub sku: String,
    pub headline: String,
    pub bullets: Vec<String>,
}

impl From<&FeatureData> for FeatureDataDto {
    fn from(d: &FeatureData) -> Self {
        Self {
            sku: d.sku().to_string(),
            headline: d.headline().to_string(),
            bullets: d.bullets().to_vec(),
        }
    }
}
