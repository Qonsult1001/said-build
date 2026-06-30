//! The `Lead` aggregate and `UserPreferences` value object.

use sca_core::{AccountId, LeadId};

use crate::error::{SearchError, SearchResult};

/// A search lead — a result a user saved/captured. Private fields; built via
/// `capture`, which validates the title is non-empty and the score is in range.
#[derive(Debug, Clone)]
pub struct Lead {
    id: LeadId,
    owner: AccountId,
    title: String,
    keywords: Vec<String>,
    score: u32,
}

impl Lead {
    pub fn capture(
        id: LeadId,
        owner: AccountId,
        title: impl Into<String>,
        keywords: Vec<String>,
        score: u32,
    ) -> SearchResult<Self> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(SearchError::Validation("empty lead title".into()));
        }
        if score > 100 {
            return Err(SearchError::Validation("score must be 0..=100".into()));
        }
        Ok(Self {
            id,
            owner,
            title,
            keywords,
            score,
        })
    }

    pub fn id(&self) -> &LeadId {
        &self.id
    }
    pub fn owner(&self) -> &AccountId {
        &self.owner
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn keywords(&self) -> &[String] {
        &self.keywords
    }
    pub fn score(&self) -> u32 {
        self.score
    }

    /// Does this lead match a lowercase query term in its title or keywords?
    pub fn matches(&self, needle: &str) -> bool {
        let n = needle.to_lowercase();
        self.title.to_lowercase().contains(&n)
            || self.keywords.iter().any(|k| k.to_lowercase().contains(&n))
    }
}

/// Per-user search preferences (a value object keyed by owner).
#[derive(Debug, Clone)]
pub struct UserPreferences {
    owner: AccountId,
    page_size: u32,
    safe_search: bool,
}

impl UserPreferences {
    pub fn new(owner: AccountId, page_size: u32, safe_search: bool) -> SearchResult<Self> {
        if page_size == 0 || page_size > 100 {
            return Err(SearchError::Validation("page_size must be 1..=100".into()));
        }
        Ok(Self {
            owner,
            page_size,
            safe_search,
        })
    }

    pub fn owner(&self) -> &AccountId {
        &self.owner
    }
    pub fn page_size(&self) -> u32 {
        self.page_size
    }
    pub fn safe_search(&self) -> bool {
        self.safe_search
    }
}
