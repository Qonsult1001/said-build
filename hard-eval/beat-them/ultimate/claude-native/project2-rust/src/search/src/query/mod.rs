//! Search use case.
//!
//! Canonical flow: validate (min query length) → authorize (public search, so
//! the authorize step is a no-op identity pass) → query the index → map to DTO →
//! return.

use crate::dto::SearchHitDto;
use crate::error::{SearchError, SearchResult};
use crate::ports::SearchIndex;
use std::sync::Arc;

const MIN_QUERY_LEN: usize = 2;
const DEFAULT_LIMIT: usize = 20;

pub struct SearchService {
    index: Arc<dyn SearchIndex>,
}

impl SearchService {
    pub fn new(index: Arc<dyn SearchIndex>) -> Self {
        Self { index }
    }

    /// Execute a public search query.
    pub fn search(&self, query: &str) -> SearchResult<Vec<SearchHitDto>> {
        // 1. validate
        let trimmed = query.trim();
        if trimmed.len() < MIN_QUERY_LEN {
            return Err(SearchError::QueryTooShort { min: MIN_QUERY_LEN });
        }

        // 2. authorize (search is public — no auth gate)

        // 3. query
        let hits = self.index.search(trimmed, DEFAULT_LIMIT);

        // 4/5. map to DTO + return
        Ok(hits.iter().map(SearchHitDto::from).collect())
    }
}
