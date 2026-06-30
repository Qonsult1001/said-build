//! Accounts DTOs — the only shapes that cross the surface boundary.
//!
//! DTOs never leak the aggregate: there is no `password` field, and the mapping
//! is one-way (aggregate → DTO). The surface sees `UserProfileDto`/`SessionDto`,
//! never `Account`.

use crate::model::Account;
use core::Token;
use serde::{Deserialize, Serialize};

/// What a successful login returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDto {
    pub token: String,
    pub user_id: String,
}

impl SessionDto {
    pub fn new(token: &Token, account: &Account) -> Self {
        Self {
            token: token.value().to_string(),
            user_id: account.id().to_string(),
        }
    }
}

/// The public projection of the user profile (no secret).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfileDto {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
}

impl From<&Account> for UserProfileDto {
    fn from(a: &Account) -> Self {
        Self {
            user_id: a.id().to_string(),
            username: a.username().to_string(),
            display_name: a.display_name().to_string(),
        }
    }
}
