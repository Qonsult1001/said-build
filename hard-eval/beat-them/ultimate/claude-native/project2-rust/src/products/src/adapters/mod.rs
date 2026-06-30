//! Products adapters — in-memory impls of the Products ports.

use crate::model::{FeatureData, Product};
use crate::ports::{FeatureDataRepository, ProductRepository};
use std::collections::HashMap;
use std::sync::Mutex;

/// In-memory product catalogue keyed by sku.
#[derive(Default)]
pub struct InMemoryProductRepository {
    by_sku: Mutex<HashMap<String, Product>>,
}

impl InMemoryProductRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProductRepository for InMemoryProductRepository {
    fn by_sku(&self, sku: &str) -> Option<Product> {
        self.by_sku.lock().unwrap().get(sku).cloned()
    }

    fn all(&self) -> Vec<Product> {
        self.by_sku.lock().unwrap().values().cloned().collect()
    }

    fn save(&self, product: Product) -> Product {
        self.by_sku
            .lock()
            .unwrap()
            .insert(product.sku().to_string(), product.clone());
        product
    }
}

/// In-memory feature data keyed by sku.
#[derive(Default)]
pub struct InMemoryFeatureDataRepository {
    by_sku: Mutex<HashMap<String, FeatureData>>,
}

impl InMemoryFeatureDataRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

impl FeatureDataRepository for InMemoryFeatureDataRepository {
    fn by_sku(&self, sku: &str) -> Option<FeatureData> {
        self.by_sku.lock().unwrap().get(sku).cloned()
    }

    fn save(&self, data: FeatureData) -> FeatureData {
        self.by_sku
            .lock()
            .unwrap()
            .insert(data.sku().to_string(), data.clone());
        data
    }
}
