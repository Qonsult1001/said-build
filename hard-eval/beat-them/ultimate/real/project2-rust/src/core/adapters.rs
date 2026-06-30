//! Kernel adapters — concrete, reusable `impl`s of the kernel ports.
//!
//! These are the only place in Ring 1 that holds mutable state. They are
//! interior-mutable (`RwLock`) so a `&dyn Repository` is shareable without the
//! service owning `&mut`, mirroring how a real DB pool hands out shared access.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use crate::error::{CoreError, CoreResult};
use crate::ports::{Clock, IdGenerator, Repository};

/// A thread-safe in-memory `Repository`. Generic over any hashable id and any
/// cloneable aggregate — every capability reuses this instead of re-writing a
/// `HashMap` wrapper (the dedup-at-the-store-boundary rule).
pub struct InMemoryRepository<Id, T> {
    store: RwLock<HashMap<Id, T>>,
}

impl<Id, T> InMemoryRepository<Id, T> {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }
}

impl<Id, T> Default for InMemoryRepository<Id, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Id, T> Repository<Id, T> for InMemoryRepository<Id, T>
where
    Id: Eq + Hash + Send + Sync,
    T: Clone + Send + Sync,
{
    fn upsert(&self, id: Id, entity: T) -> CoreResult<()> {
        let mut guard = self
            .store
            .write()
            .map_err(|_| CoreError::Repository("write lock poisoned".into()))?;
        guard.insert(id, entity);
        Ok(())
    }

    fn get(&self, id: &Id) -> CoreResult<Option<T>> {
        let guard = self
            .store
            .read()
            .map_err(|_| CoreError::Repository("read lock poisoned".into()))?;
        Ok(guard.get(id).cloned())
    }

    fn list(&self) -> CoreResult<Vec<T>> {
        let guard = self
            .store
            .read()
            .map_err(|_| CoreError::Repository("read lock poisoned".into()))?;
        Ok(guard.values().cloned().collect())
    }

    fn remove(&self, id: &Id) -> CoreResult<bool> {
        let mut guard = self
            .store
            .write()
            .map_err(|_| CoreError::Repository("write lock poisoned".into()))?;
        Ok(guard.remove(id).is_some())
    }
}

/// A monotonic, prefix-tagged id generator. Deterministic enough to assert on in
/// tests, opaque enough for a demo token. A production adapter would back this
/// with a CSPRNG; the port lets us swap without touching any service.
pub struct SequentialIdGenerator {
    prefix: &'static str,
    counter: AtomicU64,
}

impl SequentialIdGenerator {
    pub fn new(prefix: &'static str) -> Self {
        Self {
            prefix,
            counter: AtomicU64::new(1),
        }
    }
}

impl IdGenerator for SequentialIdGenerator {
    fn new_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{}-{:08}", self.prefix, n)
    }
}

/// A fixed clock — explicit state, no ambient `SystemTime`. The surface injects
/// a real time source; tests inject a frozen instant.
pub struct FixedClock(pub i64);

impl Clock for FixedClock {
    fn now_unix(&self) -> i64 {
        self.0
    }
}
