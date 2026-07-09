//! Products capability (Ring 2).
//!
//! Owns product catalogue and feature data. Same vertical-slice shape as the
//! other three capabilities: domain → service → adapter → binding.

mod error;
mod model;
mod ports;

pub mod adapters;
pub mod dto;
pub mod catalog;
pub mod feature_data;

pub use error::{ProductsError, ProductsResult};
pub use model::{FeatureData, Product};
pub use ports::{FeatureDataRepository, ProductRepository};

use adapters::{InMemoryFeatureDataRepository, InMemoryProductRepository};
use catalog::ProductService;
use feature_data::FeatureDataService;
use std::sync::Arc;

/// Factory: wire the Products services over the default in-memory adapters.
pub fn build_default() -> (ProductService, FeatureDataService) {
    let products: Arc<dyn ProductRepository> = Arc::new(InMemoryProductRepository::new());
    let features: Arc<dyn FeatureDataRepository> = Arc::new(InMemoryFeatureDataRepository::new());
    (
        ProductService::new(products),
        FeatureDataService::new(features),
    )
}
