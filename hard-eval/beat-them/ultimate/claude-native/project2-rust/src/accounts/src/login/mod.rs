//! Login use case: token login.
//!
//! Canonical service flow — validate → authorize → query/persist → map to DTO →
//! return. Login is the one entry that does not pre-authorize (it establishes
//! auth), so its "authorize" step is the credential check itself.

use crate::dto::SessionDto;
use crate::error::{AccountsError, AccountsResult};
use crate::model::Credentials;
use crate::ports::{AccountRepository, SessionStore};
use core::CoreError;
use std::sync::Arc;

pub struct LoginService {
    accounts: Arc<dyn AccountRepository>,
    sessions: Arc<dyn SessionStore>,
}

impl LoginService {
    pub fn new(accounts: Arc<dyn AccountRepository>, sessions: Arc<dyn SessionStore>) -> Self {
        Self { accounts, sessions }
    }

    /// Exchange credentials for a session token.
    pub fn login(&self, creds: Credentials) -> AccountsResult<SessionDto> {
        // 1. validate
        if creds.username.trim().is_empty() || creds.password.is_empty() {
            return Err(CoreError::Validation("username/password required".into()).into());
        }

        // 2. authorize (credential check establishes identity)
        let account = self
            .accounts
            .by_username(&creds.username)
            .ok_or(AccountsError::InvalidCredentials)?;
        if !account.verify_password(&creds.password) {
            return Err(AccountsError::InvalidCredentials);
        }

        // 3. persist (mint a session)
        let token = self.sessions.issue(account.id().clone());

        // 4/5. map to DTO + return
        Ok(SessionDto::new(&token, &account))
    }
}
