//! Coverage adapter — in-memory impl of the kernel `Repository` port.
//!
//! Implements `Repository<UserId, Vec<Coverage>>`; the blanket impl in `ports`
//! then makes it a `CoverageRepository`. Interior mutability via `Mutex`.

use crate::model::Coverage;
use core::{Repository, UserId};
use std::collections::HashMap;
use std::sync::Mutex;

/// In-memory coverages keyed by owning user.
#[derive(Default)]
pub struct InMemoryCoverageRepository {
    by_user: Mutex<HashMap<UserId, Vec<Coverage>>>,
}

impl InMemoryCoverageRepository {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a coverage for a user (used by surfaces/tests).
    pub fn seed(&self, coverage: Coverage) {
        self.by_user
            .lock()
            .unwrap()
            .entry(coverage.owner().clone())
            .or_default()
            .push(coverage);
    }
}

impl Repository<UserId, Vec<Coverage>> for InMemoryCoverageRepository {
    fn get(&self, key: &UserId) -> Option<Vec<Coverage>> {
        self.by_user.lock().unwrap().get(key).cloned()
    }

    fn upsert(&self, key: UserId, value: Vec<Coverage>) -> Vec<Coverage> {
        self.by_user.lock().unwrap().insert(key, value.clone());
        value
    }

    fn list(&self) -> Vec<Vec<Coverage>> {
        self.by_user.lock().unwrap().values().cloned().collect()
    }
}
