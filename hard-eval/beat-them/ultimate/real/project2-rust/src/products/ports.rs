//! Products ports — defined in this crate's domain.

use sca_core::{ProductId, Repository};

use crate::model::Product;

/// The product repository port — named role over the kernel's generic store.
pub trait ProductRepository: Repository<ProductId, Product> {}
impl<R> ProductRepository for R where R: Repository<ProductId, Product> {}
