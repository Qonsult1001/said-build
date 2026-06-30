//! Accounts capability (Ring 2).
//!
//! Owns authentication (token login) and the user profile. Vertical slice:
//! domain (`model`, `ports`, `error`) → service (`login`, `profile`) → adapter
//! (`adapters`, in-memory) → binding (`lib.rs` re-exports + factory).

mod error;
mod model;
mod ports;

pub mod adapters;
pub mod dto;
pub mod login;
pub mod profile;

pub use error::{AccountsError, AccountsResult};
pub use model::{Account, Credentials};
pub use ports::{AccountRepository, SessionStore};

use adapters::{InMemoryAccountRepository, InMemorySessionStore};
use login::LoginService;
use profile::ProfileService;
use std::sync::Arc;

/// Factory: wire the Accounts services over the default in-memory adapters.
///
/// A surface calls this and never names a concrete adapter (slice rule:
/// factory maps config → boxed/`dyn` ports).
pub fn build_default() -> (LoginService, ProfileService) {
    let accounts: Arc<dyn AccountRepository> = Arc::new(InMemoryAccountRepository::new());
    let sessions: Arc<dyn SessionStore> = Arc::new(InMemorySessionStore::new());
    let login = LoginService::new(accounts.clone(), sessions.clone());
    let profile = ProfileService::new(accounts, sessions);
    (login, profile)
}
