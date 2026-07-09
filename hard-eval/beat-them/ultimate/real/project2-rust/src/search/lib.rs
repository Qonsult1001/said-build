//! `search` capability crate (Ring 2): search + leads + user-preferences.
//!
//! Inward on `sca-core` only. Owns the `Lead` aggregate, `UserPreferences`, the
//! `LeadRepository`/`PreferencesRepository` ports, and the `SearchService`.

pub mod adapters;
pub mod api;
pub mod error;
pub mod model;
pub mod ports;
pub mod search;

pub use adapters::{InMemoryLeadRepo, InMemoryPreferencesRepo};
pub use api::SearchApi;
pub use error::{SearchError, SearchResult};
pub use model::{Lead, UserPreferences};
pub use ports::{LeadRepository, PreferencesRepository};
pub use search::{
    CaptureLeadRequest, LeadDto, PreferencesDto, SearchRequest, SearchService,
    SetPreferencesRequest,
};
