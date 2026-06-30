//! Coverage capability (Ring 2).
//!
//! Owns a user's global coverages. Same vertical-slice shape as Accounts:
//! domain (`model`, `ports`, `error`) → service (`list_coverages`) → adapter
//! (in-memory) → binding (`lib.rs` factory). Auth is enforced via the kernel
//! `AuthContext` in the service `authorize` step.

mod error;
mod model;
mod ports;

pub mod adapters;
pub mod dto;
pub mod list_coverages;

pub use error::{CoverageError, CoverageResult};
pub use model::Coverage;
pub use ports::CoverageRepository;

use adapters::InMemoryCoverageRepository;
use list_coverages::CoverageService;
use std::sync::Arc;

/// Factory: wire the Coverage service over the default in-memory adapter.
pub fn build_default() -> CoverageService {
    let repo: Arc<dyn CoverageRepository> = Arc::new(InMemoryCoverageRepository::new());
    CoverageService::new(repo)
}
