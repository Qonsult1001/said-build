//! User-preferences use case.
//!
//! Canonical flow: validate → authorize → query/persist (scoped to the user) →
//! map to DTO → return. Reading falls back to defaults when unset.

use crate::dto::PreferencesDto;
use crate::error::SearchResult;
use crate::model::UserPreferences;
use crate::ports::PreferencesRepository;
use core::AuthContext;
use std::sync::Arc;

pub struct PreferencesService {
    repo: Arc<dyn PreferencesRepository>,
}

impl PreferencesService {
    pub fn new(repo: Arc<dyn PreferencesRepository>) -> Self {
        Self { repo }
    }

    /// Read the authenticated user's preferences (defaults if never set).
    pub fn user_preferences(&self, ctx: &AuthContext) -> SearchResult<PreferencesDto> {
        let owner = ctx.require_user()?;
        let prefs = self
            .repo
            .get(owner)
            .unwrap_or_else(|| UserPreferences::defaults(owner.clone()));
        Ok(PreferencesDto::from(&prefs))
    }

    /// Update the authenticated user's preferences.
    pub fn update(
        &self,
        ctx: &AuthContext,
        page_size: u8,
        safe_search: bool,
    ) -> SearchResult<PreferencesDto> {
        let owner = ctx.require_user()?.clone();
        // domain invariant (page_size > 0) validated in-aggregate
        let prefs = UserPreferences::new(owner, page_size, safe_search)?;
        let saved = self.repo.save(prefs);
        Ok(PreferencesDto::from(&saved))
    }
}
