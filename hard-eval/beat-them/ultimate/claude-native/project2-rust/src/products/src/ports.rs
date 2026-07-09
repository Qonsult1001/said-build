//! Products ports — traits this capability owns.

use crate::model::{FeatureData, Product};

/// Catalogue persistence port.
pub trait ProductRepository: Send + Sync {
    fn by_sku(&self, sku: &str) -> Option<Product>;
    fn all(&self) -> Vec<Product>;
    fn save(&self, product: Product) -> Product;
}

/// Feature-data persistence port, keyed by product sku.
pub trait FeatureDataRepository: Send + Sync {
    fn by_sku(&self, sku: &str) -> Option<FeatureData>;
    fn save(&self, data: FeatureData) -> FeatureData;
}
