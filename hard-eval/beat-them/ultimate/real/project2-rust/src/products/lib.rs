//! `products` capability crate (Ring 2): featuredata + product.
//!
//! Inward on `sca-core` only. Owns the `Product` aggregate + `FeatureData`, the
//! `ProductRepository` port, and the `ProductService`.

pub mod adapters;
pub mod api;
pub mod catalog;
pub mod error;
pub mod model;
pub mod ports;

pub use adapters::InMemoryProductRepo;
pub use api::ProductsApi;
pub use catalog::{
    CreateProductRequest, FeatureDataDto, ProductDto, ProductService,
};
pub use error::{ProductsError, ProductsResult};
pub use model::{FeatureData, Product};
pub use ports::ProductRepository;
