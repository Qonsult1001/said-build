//! Coverage adapters.
//!
//! The coverage repository reuses the kernel's `InMemoryRepository` directly via
//! the blanket `CoverageRepository` impl, so there is no bespoke persistence
//! adapter here — only the type alias the surface wires. Keeping this module
//! present documents the layer and gives a home to a future DB-backed adapter.

use sca_core::{CoverageId, InMemoryRepository};

use crate::model::Coverage;

/// Convenience alias for the wired in-memory store of coverages.
pub type InMemoryCoverageRepo = InMemoryRepository<CoverageId, Coverage>;
