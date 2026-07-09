//! The `Coverage` aggregate — a user's global coverage entry.

use sca_core::{AccountId, CoverageId};

use crate::error::{CoverageError, CoverageResult};

/// A coverage record owned by an account (e.g. a region/plan the user is covered
/// under). Private fields; built only via `open`, which validates invariants.
#[derive(Debug, Clone)]
pub struct Coverage {
    id: CoverageId,
    owner: AccountId,
    region: String,
    limit_cents: u64,
    active: bool,
}

impl Coverage {
    pub fn open(
        id: CoverageId,
        owner: AccountId,
        region: impl Into<String>,
        limit_cents: u64,
    ) -> CoverageResult<Self> {
        let region = region.into();
        if region.trim().is_empty() {
            return Err(CoverageError::Validation("empty region".into()));
        }
        if limit_cents == 0 {
            return Err(CoverageError::Validation("coverage limit must be > 0".into()));
        }
        Ok(Self {
            id,
            owner,
            region,
            limit_cents,
            active: true,
        })
    }

    pub fn id(&self) -> &CoverageId {
        &self.id
    }
    pub fn owner(&self) -> &AccountId {
        &self.owner
    }
    pub fn region(&self) -> &str {
        &self.region
    }
    pub fn limit_cents(&self) -> u64 {
        self.limit_cents
    }
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Behaviour method — deactivation is a state transition, not a `pub` flag set.
    pub fn deactivate(&mut self) {
        self.active = false;
    }
}
