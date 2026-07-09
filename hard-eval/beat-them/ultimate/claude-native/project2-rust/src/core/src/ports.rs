//! Kernel ports — generic collaboration traits owned by the core.
//!
//! `Repository` is the persistence port every capability's service depends on as
//! `&dyn Repository<...>`. Defining it in the kernel (not in an adapter) keeps the
//! dependency pointing inward: capability services and in-memory adapters both
//! reference this trait, never each other.

/// A generic persistence port keyed by `K`, storing aggregate `T`.
///
/// Synchronous because the in-memory adapters used here do no real I/O; a real
/// backend would make this `#[async_trait]`. Services hold `&dyn Repository<..>`
/// so they are unit-testable against a stub and free of any backend dependency.
pub trait Repository<K, T>: Send + Sync {
    /// Fetch an aggregate by key, if present.
    fn get(&self, key: &K) -> Option<T>;

    /// Insert or replace an aggregate, returning the stored value.
    fn upsert(&self, key: K, value: T) -> T;

    /// Return every stored aggregate (small in-memory datasets only).
    fn list(&self) -> Vec<T>;
}
