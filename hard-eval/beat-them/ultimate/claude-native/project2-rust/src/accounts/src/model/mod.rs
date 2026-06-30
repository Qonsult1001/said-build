//! Accounts domain aggregates.
//!
//! `Account` keeps its fields private; the only way to mutate the display name is
//! through `rename`, which validates the invariant. DTOs never expose these
//! fields directly — see `dto.rs`.

use core::UserId;
use serde::{Deserialize, Serialize};

/// Login credentials supplied by a caller. A value object, validated in-service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// The Account aggregate: identity + profile + the secret used for auth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    id: UserId,
    username: String,
    /// Never leaves the aggregate; never serialized into a DTO.
    #[serde(skip)]
    password: String,
    display_name: String,
}

impl Account {
    /// Construct a registered account. `username`/`password` must be non-empty
    /// (caller validates before calling).
    pub fn register(id: UserId, username: String, password: String, display_name: String) -> Self {
        Self {
            id,
            username,
            password,
            display_name,
        }
    }

    pub fn id(&self) -> &UserId {
        &self.id
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Verify a presented password against the stored secret.
    pub fn verify_password(&self, candidate: &str) -> bool {
        self.password == candidate
    }

    /// Rename the profile, enforcing the non-empty invariant in-aggregate.
    pub fn rename(&mut self, display_name: String) -> Result<(), core::CoreError> {
        if display_name.trim().is_empty() {
            return Err(core::CoreError::Validation("display name is empty".into()));
        }
        self.display_name = display_name;
        Ok(())
    }
}
