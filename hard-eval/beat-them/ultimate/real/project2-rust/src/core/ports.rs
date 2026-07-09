//! Kernel ports — the trait seams every capability collaborates through.
//!
//! Ports live in the **domain** of the crate that owns the concept (here, the
//! kernel owns the generic persistence + identity + clock concepts). Adapters in
//! higher crates `impl` these; services depend on `&dyn Port`, never a concrete
//! adapter. `: Send + Sync` so a port is shareable across a runtime.

use crate::error::CoreResult;

/// A generic, in-memory-or-otherwise repository over an aggregate `T` keyed by a
/// stable id. Capability crates narrow this with concrete `T`/`Id`.
///
/// Kept synchronous on purpose: the kernel is WASM/offline-safe and must not
/// pull a native async runtime into Ring 1. An async adapter can wrap a port at
/// the surface if a real DB is ever introduced.
pub trait Repository<Id, T>: Send + Sync {
    /// Insert or replace by id.
    fn upsert(&self, id: Id, entity: T) -> CoreResult<()>;
    /// Fetch by id, `None` if absent.
    fn get(&self, id: &Id) -> CoreResult<Option<T>>;
    /// Return every stored aggregate (clones).
    fn list(&self) -> CoreResult<Vec<T>>;
    /// Remove by id; returns whether anything was removed.
    fn remove(&self, id: &Id) -> CoreResult<bool>;
}

/// Issues opaque, unguessable identifiers/tokens. A port so tests inject a
/// deterministic stub and production injects a CSPRNG-backed adapter — never a
/// thread-local global (constitution rule 4: state is explicit).
pub trait IdGenerator: Send + Sync {
    fn new_id(&self) -> String;
}

/// A clock port — passed explicitly so services are deterministic under test and
/// WASM-safe (no ambient `SystemTime` reach in the kernel).
pub trait Clock: Send + Sync {
    /// Unix epoch seconds.
    fn now_unix(&self) -> i64;
}
