//! List-coverages use case.
//!
//! Same canonical flow as Accounts: validate → authorize → query → map to DTO →
//! return. Here `authorize` requires an authenticated `AuthContext` and scopes
//! the query to that user, so a caller can only ever see their own coverages.

use crate::dto::CoverageDto;
use crate::error::CoverageResult;
use crate::ports::CoverageRepository;
use core::AuthContext;
use std::sync::Arc;

pub struct CoverageService {
    repo: Arc<dyn CoverageRepository>,
}

impl CoverageService {
    pub fn new(repo: Arc<dyn CoverageRepository>) -> Self {
        Self { repo }
    }

    /// Return the authenticated user's global coverages.
    pub fn user_coverages(&self, ctx: &AuthContext) -> CoverageResult<Vec<CoverageDto>> {
        // 1. validate / 2. authorize
        let user_id = ctx.require_user()?;

        // 3. query (scoped to the user)
        let coverages = self.repo.get(user_id).unwrap_or_default();

        // 4/5. map to DTO + return
        Ok(coverages.iter().map(CoverageDto::from).collect())
    }
}
