//! Accounts ports — traits this capability owns, defined in its domain.
//!
//! Adapters `impl` these; services depend on `&dyn`. Ports live here, never in
//! `adapters/`, so the dependency points inward.

use crate::model::Account;
use core::{Token, UserId};

/// Persistence port for accounts, keyed both by id and by username (login needs
/// the latter). Extends the generic kernel `Repository` conceptually but adds the
/// username lookup that login requires.
pub trait AccountRepository: Send + Sync {
    fn by_username(&self, username: &str) -> Option<Account>;
    fn by_id(&self, id: &UserId) -> Option<Account>;
    fn save(&self, account: Account) -> Account;
}

/// Session port: mints and resolves bearer tokens issued at login.
pub trait SessionStore: Send + Sync {
    fn issue(&self, user_id: UserId) -> Token;
    fn resolve(&self, token: &Token) -> Option<UserId>;
}
