//! The `login` + `userprofile` use cases — the Accounts application service.
//!
//! This module is the canonical vertical-slice shape every later capability
//! renders: **validate the request -> authorize -> query/persist via repository
//! -> map to DTO -> return the response.** The service depends on `&dyn` ports,
//! never a concrete adapter, so it is unit-testable with stubs and free of
//! native deps.

use sca_core::{AccountId, IdGenerator};

use crate::error::{AccountsError, AccountsResult};
use crate::model::Account;
use crate::ports::{AccountRepository, PasswordHasher};

/// Inbound request to authenticate. Plain data; validated by the service.
#[derive(Debug, Clone)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// Inbound request to register a new account.
#[derive(Debug, Clone)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

/// Outbound DTO — what crosses the API boundary. Never the aggregate itself, and
/// never the password hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenDto {
    pub account_id: String,
    pub token: String,
}

/// Outbound DTO for the user profile read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserProfileDto {
    pub account_id: String,
    pub email: String,
    pub display_name: String,
}

/// Orchestrates the accounts use cases over its ports. Holds no state of its own
/// beyond injected collaborators (explicit state — constitution rule 4).
pub struct AccountService<'a> {
    repo: &'a dyn AccountRepository,
    hasher: &'a dyn PasswordHasher,
    ids: &'a dyn IdGenerator,
}

impl<'a> AccountService<'a> {
    pub fn new(
        repo: &'a dyn AccountRepository,
        hasher: &'a dyn PasswordHasher,
        ids: &'a dyn IdGenerator,
    ) -> Self {
        Self { repo, hasher, ids }
    }

    /// Register a new account, returning a session token DTO.
    pub fn register(&self, req: RegisterRequest) -> AccountsResult<TokenDto> {
        // 1. validate the request
        if req.password.len() < 8 {
            return Err(AccountsError::Validation("password too short".into()));
        }
        // 2. authorize (registration is open; reject duplicate email)
        let email_taken = self
            .repo
            .list()?
            .into_iter()
            .any(|a| a.email() == req.email);
        if email_taken {
            return Err(AccountsError::Validation("email already registered".into()));
        }
        // 3. persist via repository
        let id = AccountId::new(self.ids.new_id());
        let hash = self.hasher.hash(&req.password);
        let account = Account::register(id.clone(), req.email, hash, req.display_name)?;
        self.repo.upsert(id.clone(), account)?;
        // 4. map to DTO  5. return
        Ok(TokenDto {
            account_id: id.as_str().to_string(),
            token: self.ids.new_id(),
        })
    }

    /// Authenticate, returning a session token DTO.
    pub fn login(&self, req: LoginRequest) -> AccountsResult<TokenDto> {
        // 1. validate
        if req.email.is_empty() || req.password.is_empty() {
            return Err(AccountsError::Validation("email and password required".into()));
        }
        // 2. authorize: find the account, verify the credential
        let account = self
            .repo
            .list()?
            .into_iter()
            .find(|a| a.email() == req.email)
            .ok_or(AccountsError::InvalidCredentials)?;
        let candidate = self.hasher.hash(&req.password);
        if !account.verify(&candidate) {
            return Err(AccountsError::InvalidCredentials);
        }
        // 3. (no persist on read)  4. map to DTO  5. return
        Ok(TokenDto {
            account_id: account.id().as_str().to_string(),
            token: self.ids.new_id(),
        })
    }

    /// Read a user's profile by account id.
    pub fn userprofile(&self, account_id: &AccountId) -> AccountsResult<UserProfileDto> {
        // 1. (id is the validated input)  2. authorize implicit  3. query repository
        let account = self
            .repo
            .get(account_id)?
            .ok_or_else(|| AccountsError::NotFound(account_id.to_string()))?;
        // 4. map to DTO  5. return
        Ok(UserProfileDto {
            account_id: account.id().as_str().to_string(),
            email: account.email().to_string(),
            display_name: account.display_name().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sca_core::{InMemoryRepository, SequentialIdGenerator};

    struct StubHasher;
    impl PasswordHasher for StubHasher {
        fn hash(&self, plaintext: &str) -> String {
            format!("hashed::{plaintext}")
        }
    }

    #[test]
    fn register_then_login_then_profile() {
        let repo = InMemoryRepository::<AccountId, Account>::new();
        let hasher = StubHasher;
        let ids = SequentialIdGenerator::new("acct");
        let svc = AccountService::new(&repo, &hasher, &ids);

        let tok = svc
            .register(RegisterRequest {
                email: "a@b.com".into(),
                password: "supersecret".into(),
                display_name: "Ada".into(),
            })
            .unwrap();

        let login = svc
            .login(LoginRequest {
                email: "a@b.com".into(),
                password: "supersecret".into(),
            })
            .unwrap();
        assert_eq!(login.account_id, tok.account_id);

        let profile = svc
            .userprofile(&AccountId::new(tok.account_id.clone()))
            .unwrap();
        assert_eq!(profile.email, "a@b.com");
        assert_eq!(profile.display_name, "Ada");
    }

    #[test]
    fn wrong_password_is_invalid_credentials() {
        let repo = InMemoryRepository::<AccountId, Account>::new();
        let hasher = StubHasher;
        let ids = SequentialIdGenerator::new("acct");
        let svc = AccountService::new(&repo, &hasher, &ids);
        svc.register(RegisterRequest {
            email: "a@b.com".into(),
            password: "supersecret".into(),
            display_name: "Ada".into(),
        })
        .unwrap();

        let err = svc
            .login(LoginRequest {
                email: "a@b.com".into(),
                password: "wrongpass".into(),
            })
            .unwrap_err();
        assert!(matches!(err, AccountsError::InvalidCredentials));
    }
}
