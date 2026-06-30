//! Leads use case.
//!
//! Canonical flow: validate → authorize (authenticated user) → persist/query
//! scoped to that user → map to DTO → return.

use crate::dto::LeadDto;
use crate::error::SearchResult;
use crate::model::Lead;
use crate::ports::LeadRepository;
use core::AuthContext;
use std::sync::Arc;

pub struct LeadService {
    repo: Arc<dyn LeadRepository>,
}

impl LeadService {
    pub fn new(repo: Arc<dyn LeadRepository>) -> Self {
        Self { repo }
    }

    /// Capture a new lead for the authenticated user.
    pub fn capture(&self, ctx: &AuthContext, email: String, note: String) -> SearchResult<LeadDto> {
        // 1. validate / 2. authorize
        let owner = ctx.require_user()?.clone();
        // domain invariant (valid email) validated in-aggregate
        let lead = Lead::capture(owner, email, note)?;

        // 3. persist
        let stored = self.repo.add(lead);

        // 4/5. map to DTO + return
        Ok(LeadDto::from(&stored))
    }

    /// List the authenticated user's leads.
    pub fn user_leads(&self, ctx: &AuthContext) -> SearchResult<Vec<LeadDto>> {
        let owner = ctx.require_user()?;
        let leads = self.repo.for_user(owner);
        Ok(leads.iter().map(LeadDto::from).collect())
    }
}
