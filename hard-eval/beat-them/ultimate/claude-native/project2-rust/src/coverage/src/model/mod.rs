//! Coverage domain aggregate.
//!
//! A `Coverage` is owned by a user. Fields are private; construction validates
//! the invariant that a coverage always has a non-empty name and a positive
//! limit.

use core::{CoreError, UserId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single global coverage line belonging to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coverage {
    id: Uuid,
    owner: UserId,
    name: String,
    /// Coverage limit in whole currency units; always > 0.
    limit: u64,
    active: bool,
}

impl Coverage {
    /// Create a coverage, validating invariants in-aggregate.
    pub fn new(owner: UserId, name: String, limit: u64) -> Result<Self, CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("coverage name is empty".into()));
        }
        if limit == 0 {
            return Err(CoreError::Validation("coverage limit must be positive".into()));
        }
        Ok(Self {
            id: Uuid::new_v4(),
            owner,
            name,
            limit,
            active: true,
        })
    }

    pub fn id(&self) -> &Uuid {
        &self.id
    }

    pub fn owner(&self) -> &UserId {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn limit(&self) -> u64 {
        self.limit
    }

    pub fn is_active(&self) -> bool {
        self.active
    }
}
