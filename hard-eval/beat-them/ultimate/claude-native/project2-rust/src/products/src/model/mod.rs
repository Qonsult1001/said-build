//! Products domain aggregates: product + feature data.

use core::CoreError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A catalogue product. Fields private; construction validates invariants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    id: Uuid,
    sku: String,
    name: String,
    price_cents: u64,
}

impl Product {
    pub fn new(sku: String, name: String, price_cents: u64) -> Result<Self, CoreError> {
        if sku.trim().is_empty() {
            return Err(CoreError::Validation("sku is empty".into()));
        }
        if name.trim().is_empty() {
            return Err(CoreError::Validation("product name is empty".into()));
        }
        Ok(Self {
            id: Uuid::new_v4(),
            sku,
            name,
            price_cents,
        })
    }

    pub fn id(&self) -> &Uuid {
        &self.id
    }
    pub fn sku(&self) -> &str {
        &self.sku
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn price_cents(&self) -> u64 {
        self.price_cents
    }
}

/// Marketing/feature data describing a product (one-to-one with a product sku).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureData {
    sku: String,
    headline: String,
    bullets: Vec<String>,
}

impl FeatureData {
    pub fn new(sku: String, headline: String, bullets: Vec<String>) -> Result<Self, CoreError> {
        if headline.trim().is_empty() {
            return Err(CoreError::Validation("headline is empty".into()));
        }
        Ok(Self {
            sku,
            headline,
            bullets,
        })
    }

    pub fn sku(&self) -> &str {
        &self.sku
    }
    pub fn headline(&self) -> &str {
        &self.headline
    }
    pub fn bullets(&self) -> &[String] {
        &self.bullets
    }
}
