//! Search adapters — in-memory impls of the three Search ports.

use crate::model::{Lead, SearchResultItem, UserPreferences};
use crate::ports::{LeadRepository, PreferencesRepository, SearchIndex};
use core::UserId;
use std::collections::HashMap;
use std::sync::Mutex;

/// In-memory search index over a seeded corpus of titles.
#[derive(Default)]
pub struct InMemorySearchIndex {
    corpus: Mutex<Vec<String>>,
}

impl InMemorySearchIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed the corpus (used by surfaces/tests).
    pub fn seed(&self, title: &str) {
        self.corpus.lock().unwrap().push(title.to_string());
    }
}

impl SearchIndex for InMemorySearchIndex {
    fn search(&self, query: &str, limit: usize) -> Vec<SearchResultItem> {
        let needle = query.to_lowercase();
        self.corpus
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.to_lowercase().contains(&needle))
            .take(limit)
            .map(|t| SearchResultItem::new(t.clone(), 1.0))
            .collect()
    }
}

/// In-memory lead store keyed by owning user.
#[derive(Default)]
pub struct InMemoryLeadRepository {
    by_user: Mutex<HashMap<UserId, Vec<Lead>>>,
}

impl InMemoryLeadRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

impl LeadRepository for InMemoryLeadRepository {
    fn add(&self, lead: Lead) -> Lead {
        self.by_user
            .lock()
            .unwrap()
            .entry(lead.owner().clone())
            .or_default()
            .push(lead.clone());
        lead
    }

    fn for_user(&self, owner: &UserId) -> Vec<Lead> {
        self.by_user
            .lock()
            .unwrap()
            .get(owner)
            .cloned()
            .unwrap_or_default()
    }
}

/// In-memory preferences store keyed by user.
#[derive(Default)]
pub struct InMemoryPreferencesRepository {
    by_user: Mutex<HashMap<UserId, UserPreferences>>,
}

impl InMemoryPreferencesRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PreferencesRepository for InMemoryPreferencesRepository {
    fn get(&self, owner: &UserId) -> Option<UserPreferences> {
        self.by_user.lock().unwrap().get(owner).cloned()
    }

    fn save(&self, prefs: UserPreferences) -> UserPreferences {
        self.by_user
            .lock()
            .unwrap()
            .insert(prefs.owner().clone(), prefs.clone());
        prefs
    }
}
