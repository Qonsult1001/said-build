//! Accounts adapters — in-memory `impl`s of the capability ports.
//!
//! The only layer that owns the storage representation. Uses `Mutex<HashMap>`
//! for interior mutability behind the `&self` port signatures. No real I/O.

use crate::model::Account;
use crate::ports::{AccountRepository, SessionStore};
use core::{Token, UserId};
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

/// In-memory account store, indexed by username and by id.
#[derive(Default)]
pub struct InMemoryAccountRepository {
    by_username: Mutex<HashMap<String, Account>>,
}

impl InMemoryAccountRepository {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a demo account (used by surfaces/tests to have a login target).
    pub fn seed(&self, username: &str, password: &str, display_name: &str) -> UserId {
        let id = UserId::new();
        let account = Account::register(
            id.clone(),
            username.to_string(),
            password.to_string(),
            display_name.to_string(),
        );
        self.save(account);
        id
    }
}

impl AccountRepository for InMemoryAccountRepository {
    fn by_username(&self, username: &str) -> Option<Account> {
        self.by_username.lock().unwrap().get(username).cloned()
    }

    fn by_id(&self, id: &UserId) -> Option<Account> {
        self.by_username
            .lock()
            .unwrap()
            .values()
            .find(|a| a.id() == id)
            .cloned()
    }

    fn save(&self, account: Account) -> Account {
        self.by_username
            .lock()
            .unwrap()
            .insert(account.username().to_string(), account.clone());
        account
    }
}

/// In-memory session store mapping opaque tokens → user ids.
#[derive(Default)]
pub struct InMemorySessionStore {
    tokens: Mutex<HashMap<String, UserId>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SessionStore for InMemorySessionStore {
    fn issue(&self, user_id: UserId) -> Token {
        let token = Token::new(Uuid::new_v4().to_string());
        self.tokens
            .lock()
            .unwrap()
            .insert(token.value().to_string(), user_id);
        token
    }

    fn resolve(&self, token: &Token) -> Option<UserId> {
        self.tokens.lock().unwrap().get(token.value()).cloned()
    }
}
