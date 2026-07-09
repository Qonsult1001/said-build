//! Search capability (Ring 2).
//!
//! Owns search, leads, and user-preferences. Three vertical slices sharing one
//! domain. Same shape as Accounts/Coverage: domain → service → adapter → binding.

mod error;
mod model;
mod ports;

pub mod adapters;
pub mod dto;
pub mod leads;
pub mod preferences;
pub mod query;

pub use error::{SearchError, SearchResult};
pub use model::{Lead, SearchResultItem, UserPreferences};
pub use ports::{LeadRepository, PreferencesRepository, SearchIndex};

use adapters::{InMemoryLeadRepository, InMemoryPreferencesRepository, InMemorySearchIndex};
use leads::LeadService;
use preferences::PreferencesService;
use query::SearchService;
use std::sync::Arc;

/// Factory: wire the three Search services over the default in-memory adapters.
pub fn build_default() -> (SearchService, LeadService, PreferencesService) {
    let index: Arc<dyn SearchIndex> = Arc::new(InMemorySearchIndex::new());
    let leads: Arc<dyn LeadRepository> = Arc::new(InMemoryLeadRepository::new());
    let prefs: Arc<dyn PreferencesRepository> = Arc::new(InMemoryPreferencesRepository::new());
    (
        SearchService::new(index),
        LeadService::new(leads),
        PreferencesService::new(prefs),
    )
}
