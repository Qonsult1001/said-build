//! Search DTOs — projections crossing the surface boundary.

use crate::model::{Lead, SearchResultItem, UserPreferences};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHitDto {
    pub id: String,
    pub title: String,
    pub score: f32,
}

impl From<&SearchResultItem> for SearchHitDto {
    fn from(i: &SearchResultItem) -> Self {
        Self {
            id: i.id().to_string(),
            title: i.title().to_string(),
            score: i.score(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeadDto {
    pub id: String,
    pub email: String,
    pub note: String,
}

impl From<&Lead> for LeadDto {
    fn from(l: &Lead) -> Self {
        Self {
            id: l.id().to_string(),
            email: l.email().to_string(),
            note: l.note().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferencesDto {
    pub page_size: u8,
    pub safe_search: bool,
}

impl From<&UserPreferences> for PreferencesDto {
    fn from(p: &UserPreferences) -> Self {
        Self {
            page_size: p.page_size(),
            safe_search: p.safe_search(),
        }
    }
}
