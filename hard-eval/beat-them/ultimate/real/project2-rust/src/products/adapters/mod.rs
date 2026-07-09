//! Products adapters — the in-memory store alias the surface wires.

use sca_core::{InMemoryRepository, ProductId};

use crate::model::Product;

pub type InMemoryProductRepo = InMemoryRepository<ProductId, Product>;
