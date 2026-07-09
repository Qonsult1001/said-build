//! Search domain aggregates: search result, lead, user preferences.

use core::{CoreError, UserId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single hit returned from the search index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    id: Uuid,
    title: String,
    score: f32,
}

impl SearchResultItem {
    pub fn new(title: String, score: f32) -> Self {
        Self {
            id: Uuid::new_v4(),
            title,
            score,
        }
    }

    pub fn id(&self) -> &Uuid {
        &self.id
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn score(&self) -> f32 {
        self.score
    }
}

/// A captured lead belonging to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lead {
    id: Uuid,
    owner: UserId,
    email: String,
    note: String,
}

impl Lead {
    /// Create a lead, validating the email invariant in-aggregate.
    pub fn capture(owner: UserId, email: String, note: String) -> Result<Self, CoreError> {
        if !email.contains('@') {
            return Err(CoreError::Validation("invalid lead email".into()));
        }
        Ok(Self {
            id: Uuid::new_v4(),
            owner,
            email,
            note,
        })
    }

    pub fn id(&self) -> &Uuid {
        &self.id
    }
    pub fn owner(&self) -> &UserId {
        &self.owner
    }
    pub fn email(&self) -> &str {
        &self.email
    }
    pub fn note(&self) -> &str {
        &self.note
    }
}

/// A user's search preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    owner: UserId,
    page_size: u8,
    safe_search: bool,
}

impl UserPreferences {
    pub fn new(owner: UserId, page_size: u8, safe_search: bool) -> Result<Self, CoreError> {
        if page_size == 0 {
            return Err(CoreError::Validation("page size must be positive".into()));
        }
        Ok(Self {
            owner,
            page_size,
            safe_search,
        })
    }

    /// Sensible defaults for a user who has never set preferences.
    pub fn defaults(owner: UserId) -> Self {
        Self {
            owner,
            page_size: 20,
            safe_search: true,
        }
    }

    pub fn owner(&self) -> &UserId {
        &self.owner
    }
    pub fn page_size(&self) -> u8 {
        self.page_size
    }
    pub fn safe_search(&self) -> bool {
        self.safe_search
    }
}
