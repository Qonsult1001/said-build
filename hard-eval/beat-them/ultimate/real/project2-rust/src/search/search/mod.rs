//! The `search` + `leads` + `user-preferences` use cases.
//!
//! Renders the blueprint: validate -> authorize -> query/persist via repository
//! -> map to DTO -> return. The module-specific 20% is the relevance filter and
//! the page-size honoured from the user's preferences.

use sca_core::{AccountId, IdGenerator, LeadId};

use crate::error::{SearchError, SearchResult};
use crate::model::{Lead, UserPreferences};
use crate::ports::{LeadRepository, PreferencesRepository};

#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub owner_id: String,
    pub query: String,
}

#[derive(Debug, Clone)]
pub struct CaptureLeadRequest {
    pub owner_id: String,
    pub title: String,
    pub keywords: Vec<String>,
    pub score: u32,
}

#[derive(Debug, Clone)]
pub struct SetPreferencesRequest {
    pub owner_id: String,
    pub page_size: u32,
    pub safe_search: bool,
}

/// Outbound DTO — never the aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeadDto {
    pub id: String,
    pub title: String,
    pub score: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferencesDto {
    pub owner_id: String,
    pub page_size: u32,
    pub safe_search: bool,
}

impl LeadDto {
    fn from_aggregate(l: &Lead) -> Self {
        Self {
            id: l.id().as_str().to_string(),
            title: l.title().to_string(),
            score: l.score(),
        }
    }
}

pub struct SearchService<'a> {
    leads: &'a dyn LeadRepository,
    prefs: &'a dyn PreferencesRepository,
    ids: &'a dyn IdGenerator,
}

impl<'a> SearchService<'a> {
    pub fn new(
        leads: &'a dyn LeadRepository,
        prefs: &'a dyn PreferencesRepository,
        ids: &'a dyn IdGenerator,
    ) -> Self {
        Self { leads, prefs, ids }
    }

    /// Capture (persist) a lead for a user.
    pub fn capture_lead(&self, req: CaptureLeadRequest) -> SearchResult<LeadDto> {
        // 1. validate
        if req.owner_id.is_empty() {
            return Err(SearchError::Validation("owner required".into()));
        }
        // 2. authorize (owner scope)
        let owner = AccountId::new(req.owner_id);
        // 3. persist via repository
        let id = LeadId::new(self.ids.new_id());
        let lead = Lead::capture(id.clone(), owner, req.title, req.keywords, req.score)?;
        self.leads.upsert(id, lead.clone())?;
        // 4. map  5. return
        Ok(LeadDto::from_aggregate(&lead))
    }

    /// Set a user's search preferences (persist).
    pub fn set_preferences(&self, req: SetPreferencesRequest) -> SearchResult<PreferencesDto> {
        if req.owner_id.is_empty() {
            return Err(SearchError::Validation("owner required".into()));
        }
        let owner = AccountId::new(req.owner_id);
        let prefs = UserPreferences::new(owner.clone(), req.page_size, req.safe_search)?;
        self.prefs.upsert(owner.clone(), prefs.clone())?;
        Ok(PreferencesDto {
            owner_id: owner.as_str().to_string(),
            page_size: prefs.page_size(),
            safe_search: prefs.safe_search(),
        })
    }

    /// Run a search over the user's leads, honouring their page-size preference.
    pub fn search(&self, req: SearchRequest) -> SearchResult<Vec<LeadDto>> {
        // 1. validate
        if req.query.trim().is_empty() {
            return Err(SearchError::Validation("empty query".into()));
        }
        // 2. authorize: scope to owner; resolve page size from preferences
        let owner = AccountId::new(req.owner_id);
        let page_size = self
            .prefs
            .get(&owner)?
            .map(|p| p.page_size())
            .unwrap_or(10) as usize;
        // 3. query repository + filter by relevance
        let mut hits: Vec<Lead> = self
            .leads
            .list()?
            .into_iter()
            .filter(|l| l.owner() == &owner && l.matches(&req.query))
            .collect();
        // rank: highest score first, then stable by id
        hits.sort_by(|a, b| {
            b.score()
                .cmp(&a.score())
                .then_with(|| a.id().as_str().cmp(b.id().as_str()))
        });
        // 4. map (paginated) 5. return
        Ok(hits
            .iter()
            .take(page_size)
            .map(LeadDto::from_aggregate)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sca_core::{InMemoryRepository, SequentialIdGenerator};

    fn svc<'a>(
        leads: &'a InMemoryRepository<LeadId, Lead>,
        prefs: &'a InMemoryRepository<AccountId, UserPreferences>,
        ids: &'a SequentialIdGenerator,
    ) -> SearchService<'a> {
        SearchService::new(leads, prefs, ids)
    }

    #[test]
    fn search_ranks_and_honours_page_size() {
        let leads = InMemoryRepository::<LeadId, Lead>::new();
        let prefs = InMemoryRepository::<AccountId, UserPreferences>::new();
        let ids = SequentialIdGenerator::new("lead");
        let s = svc(&leads, &prefs, &ids);

        for (t, kw, sc) in [
            ("Rust jobs", vec!["rust".to_string()], 90),
            ("Rust crate", vec!["rust".to_string()], 95),
            ("Go jobs", vec!["go".to_string()], 50),
        ] {
            s.capture_lead(CaptureLeadRequest {
                owner_id: "u1".into(),
                title: t.into(),
                keywords: kw,
                score: sc,
            })
            .unwrap();
        }
        s.set_preferences(SetPreferencesRequest {
            owner_id: "u1".into(),
            page_size: 1,
            safe_search: true,
        })
        .unwrap();

        let out = s
            .search(SearchRequest {
                owner_id: "u1".into(),
                query: "rust".into(),
            })
            .unwrap();
        assert_eq!(out.len(), 1, "page_size=1 honoured");
        assert_eq!(out[0].title, "Rust crate", "highest score ranked first");
    }
}
