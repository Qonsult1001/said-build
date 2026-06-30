//! `coverage` capability crate (Ring 2): a user's global coverages.
//!
//! Inward on `sca-core` only. Owns the `Coverage` aggregate, the
//! `CoverageRepository` port, the `CoverageService`, and the in-memory repo alias.

pub mod adapters;
pub mod api;
pub mod coverages;
pub mod error;
pub mod model;
pub mod ports;

pub use adapters::InMemoryCoverageRepo;
pub use api::CoverageApi;
pub use coverages::{CoverageDto, CoverageService, CreateCoverageRequest};
pub use error::{CoverageError, CoverageResult};
pub use model::Coverage;
pub use ports::CoverageRepository;
