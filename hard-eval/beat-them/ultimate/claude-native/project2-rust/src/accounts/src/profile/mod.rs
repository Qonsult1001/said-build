//! Profile use case: read the authenticated user's profile.
//!
//! Full canonical flow including the authorize step (resolve a token → user).

use crate::dto::UserProfileDto;
use crate::error::AccountsResult;
use crate::ports::{AccountRepository, SessionStore};
use core::{CoreError, Token};
use std::sync::Arc;

pub struct ProfileService {
    accounts: Arc<dyn AccountRepository>,
    sessions: Arc<dyn SessionStore>,
}

impl ProfileService {
    pub fn new(accounts: Arc<dyn AccountRepository>, sessions: Arc<dyn SessionStore>) -> Self {
        Self { accounts, sessions }
    }

    /// Return the profile of the user identified by `token`.
    pub fn user_profile(&self, token: &Token) -> AccountsResult<UserProfileDto> {
        // 1. validate
        if token.value().trim().is_empty() {
            return Err(CoreError::Validation("token required".into()).into());
        }

        // 2. authorize
        let user_id = self
            .sessions
            .resolve(token)
            .ok_or_else(|| CoreError::Unauthorized("invalid session".into()))?;

        // 3. query
        let account = self.accounts.by_id(&user_id).ok_or_else(|| CoreError::NotFound {
            entity: "account",
            id: user_id.to_string(),
        })?;

        // 4/5. map to DTO + return
        Ok(UserProfileDto::from(&account))
    }
}
