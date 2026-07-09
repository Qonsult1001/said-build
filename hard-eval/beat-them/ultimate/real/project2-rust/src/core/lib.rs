//! `sca-core` — the kernel (Ring 1).
//!
//! Pure, target-agnostic, WASM-safe: domain types, the port traits every
//! capability collaborates through, the typed error, and the reusable in-memory
//! adapters. No capability crate's internals, no surface, no native async
//! runtime reach in here. Capability crates depend *inward* on this and nothing
//! sideways.

pub mod adapters;
pub mod error;
pub mod model;
pub mod ports;

// Deliberate public API surface, re-exported from the crate root.
pub use error::{CoreError, CoreResult, FailureClass};
pub use model::{AccountId, CoverageId, LeadId, ProductId};
pub use ports::{Clock, IdGenerator, Repository};
pub use adapters::{FixedClock, InMemoryRepository, SequentialIdGenerator};
