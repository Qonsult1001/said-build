//! Search ports — traits this capability owns.

use crate::model::{Lead, SearchResultItem, UserPreferences};
use core::UserId;

/// Read-only search index port.
pub trait SearchIndex: Send + Sync {
    fn search(&self, query: &str, limit: usize) -> Vec<SearchResultItem>;
}

/// Persistence port for leads, scoped per user.
pub trait LeadRepository: Send + Sync {
    fn add(&self, lead: Lead) -> Lead;
    fn for_user(&self, owner: &UserId) -> Vec<Lead>;
}

/// Persistence port for user preferences.
pub trait PreferencesRepository: Send + Sync {
    fn get(&self, owner: &UserId) -> Option<UserPreferences>;
    fn save(&self, prefs: UserPreferences) -> UserPreferences;
}
