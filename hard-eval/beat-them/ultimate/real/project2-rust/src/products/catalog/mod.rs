//! The `product` + `featuredata` use cases — the Products application service.
//!
//! Renders the blueprint: validate -> authorize -> query/persist via repository
//! -> map to DTO -> return. The module-specific 20% is the featured filter and
//! the FeatureData projection.

use sca_core::{IdGenerator, ProductId};

use crate::error::{ProductsError, ProductsResult};
use crate::model::{FeatureData, Product};
use crate::ports::ProductRepository;

#[derive(Debug, Clone)]
pub struct CreateProductRequest {
    pub name: String,
    pub price_cents: u64,
    pub headline: String,
    pub bullets: Vec<String>,
    pub featured: bool,
}

/// Outbound product DTO — never the aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductDto {
    pub id: String,
    pub name: String,
    pub price_cents: u64,
    pub featured: bool,
}

/// Outbound feature-data DTO (the `featuredata` read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureDataDto {
    pub product_id: String,
    pub headline: String,
    pub bullets: Vec<String>,
}

impl ProductDto {
    fn from_aggregate(p: &Product) -> Self {
        Self {
            id: p.id().as_str().to_string(),
            name: p.name().to_string(),
            price_cents: p.price_cents(),
            featured: p.is_featured(),
        }
    }
}

pub struct ProductService<'a> {
    repo: &'a dyn ProductRepository,
    ids: &'a dyn IdGenerator,
}

impl<'a> ProductService<'a> {
    pub fn new(repo: &'a dyn ProductRepository, ids: &'a dyn IdGenerator) -> Self {
        Self { repo, ids }
    }

    /// Create (persist) a product with its feature data.
    pub fn create(&self, req: CreateProductRequest) -> ProductsResult<ProductDto> {
        // 1. validate
        if req.name.trim().is_empty() {
            return Err(ProductsError::Validation("name required".into()));
        }
        // 2. authorize (catalogue writes are operator-scoped; open in the demo)
        // 3. persist via repository
        let id = ProductId::new(self.ids.new_id());
        let feature = FeatureData {
            headline: req.headline,
            bullets: req.bullets,
            featured: req.featured,
        };
        let product = Product::list(id.clone(), req.name, req.price_cents, feature)?;
        self.repo.upsert(id, product.clone())?;
        // 4. map  5. return
        Ok(ProductDto::from_aggregate(&product))
    }

    /// List products, optionally only the featured ones.
    pub fn product(&self, only_featured: bool) -> ProductsResult<Vec<ProductDto>> {
        // 1. (flag is the validated input)  2. authorize implicit  3. query
        let mut out: Vec<ProductDto> = self
            .repo
            .list()?
            .iter()
            .filter(|p| !only_featured || p.is_featured())
            .map(ProductDto::from_aggregate)
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        // 4. (mapped)  5. return
        Ok(out)
    }

    /// Read the feature data projection for one product.
    pub fn featuredata(&self, product_id: &str) -> ProductsResult<FeatureDataDto> {
        // 1. validate id  3. query repository
        let id = ProductId::new(product_id.to_string());
        let product = self
            .repo
            .get(&id)?
            .ok_or_else(|| ProductsError::NotFound(product_id.to_string()))?;
        // 4. map aggregate -> projection DTO  5. return
        Ok(FeatureDataDto {
            product_id: product.id().as_str().to_string(),
            headline: product.feature().headline.clone(),
            bullets: product.feature().bullets.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sca_core::{InMemoryRepository, SequentialIdGenerator};

    #[test]
    fn featured_filter_and_featuredata() {
        let repo = InMemoryRepository::<ProductId, Product>::new();
        let ids = SequentialIdGenerator::new("prod");
        let svc = ProductService::new(&repo, &ids);

        let p = svc
            .create(CreateProductRequest {
                name: "Pro Plan".into(),
                price_cents: 4900,
                headline: "Everything unlocked".into(),
                bullets: vec!["Unlimited".into()],
                featured: true,
            })
            .unwrap();
        svc.create(CreateProductRequest {
            name: "Basic".into(),
            price_cents: 900,
            headline: "Starter".into(),
            bullets: vec![],
            featured: false,
        })
        .unwrap();

        let featured = svc.product(true).unwrap();
        assert_eq!(featured.len(), 1);
        assert_eq!(featured[0].name, "Pro Plan");

        let fd = svc.featuredata(&p.id).unwrap();
        assert_eq!(fd.headline, "Everything unlocked");
    }
}
