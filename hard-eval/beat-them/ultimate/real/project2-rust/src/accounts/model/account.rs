//! The `Account` aggregate.
//!
//! Fields are private; the only way to build one is `register`, which enforces
//! the invariants. A public field would let a caller bypass the email/password
//! checks — forbidden by the violation table.

use sca_core::AccountId;

use crate::error::{AccountsError, AccountsResult};

/// A registered account. The password is held as an opaque hash, never plaintext.
#[derive(Debug, Clone)]
pub struct Account {
    id: AccountId,
    email: String,
    password_hash: String,
    display_name: String,
}

impl Account {
    /// Construct a valid account, validating invariants in-aggregate.
    pub fn register(
        id: AccountId,
        email: impl Into<String>,
        password_hash: impl Into<String>,
        display_name: impl Into<String>,
    ) -> AccountsResult<Self> {
        let email = email.into();
        let password_hash = password_hash.into();
        let display_name = display_name.into();

        if !email.contains('@') || email.len() < 3 {
            return Err(AccountsError::Validation(format!("malformed email: {email}")));
        }
        if password_hash.is_empty() {
            return Err(AccountsError::Validation("empty password hash".into()));
        }
        if display_name.trim().is_empty() {
            return Err(AccountsError::Validation("empty display name".into()));
        }

        Ok(Self {
            id,
            email,
            password_hash,
            display_name,
        })
    }

    pub fn id(&self) -> &AccountId {
        &self.id
    }
    pub fn email(&self) -> &str {
        &self.email
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Constant-shape credential check. Returns whether the supplied hash matches.
    pub fn verify(&self, candidate_hash: &str) -> bool {
        self.password_hash == candidate_hash
    }
}
