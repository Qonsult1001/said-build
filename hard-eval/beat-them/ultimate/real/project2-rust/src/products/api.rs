//! Products API handler — thin binding over the service.

use sca_core::IdGenerator;

use crate::catalog::{
    CreateProductRequest, FeatureDataDto, ProductDto, ProductService,
};
use crate::error::ProductsResult;
use crate::ports::ProductRepository;

pub struct ProductsApi<'a> {
    service: ProductService<'a>,
}

impl<'a> ProductsApi<'a> {
    pub fn new(repo: &'a dyn ProductRepository, ids: &'a dyn IdGenerator) -> Self {
        Self {
            service: ProductService::new(repo, ids),
        }
    }

    pub fn create(&self, req: CreateProductRequest) -> ProductsResult<ProductDto> {
        self.service.create(req)
    }

    pub fn product(&self, only_featured: bool) -> ProductsResult<Vec<ProductDto>> {
        self.service.product(only_featured)
    }

    pub fn featuredata(&self, product_id: &str) -> ProductsResult<FeatureDataDto> {
        self.service.featuredata(product_id)
    }
}
