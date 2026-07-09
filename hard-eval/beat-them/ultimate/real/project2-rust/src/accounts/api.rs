//! Accounts API handler — the thin binding over the service.
//!
//! Handlers return `Result<Dto, AccountsError>`; they wire ports to the service
//! and translate, holding NO business logic (thin-surface-over-core). In a real
//! deployment an HTTP framework calls these; here they are the call surface.

use sca_core::{AccountId, IdGenerator};

use crate::error::AccountsResult;
use crate::login::{
    AccountService, LoginRequest, RegisterRequest, TokenDto, UserProfileDto,
};
use crate::ports::{AccountRepository, PasswordHasher};

/// Bundles the injected collaborators so each handler stays a one-liner over the
/// service. Constructed once at the surface and shared.
pub struct AccountsApi<'a> {
    service: AccountService<'a>,
}

impl<'a> AccountsApi<'a> {
    pub fn new(
        repo: &'a dyn AccountRepository,
        hasher: &'a dyn PasswordHasher,
        ids: &'a dyn IdGenerator,
    ) -> Self {
        Self {
            service: AccountService::new(repo, hasher, ids),
        }
    }

    pub fn register(&self, req: RegisterRequest) -> AccountsResult<TokenDto> {
        self.service.register(req)
    }

    pub fn login(&self, req: LoginRequest) -> AccountsResult<TokenDto> {
        self.service.login(req)
    }

    pub fn userprofile(&self, account_id: &str) -> AccountsResult<UserProfileDto> {
        self.service.userprofile(&AccountId::new(account_id.to_string()))
    }
}
