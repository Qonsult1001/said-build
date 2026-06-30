//! Accounts ports — defined in this crate's domain, `impl`'d by adapters.

use sca_core::{AccountId, Repository};

use crate::model::Account;

/// The accounts repository port. A type alias over the kernel's generic
/// `Repository` so the in-memory adapter is reused, while the service depends on
/// this named role rather than the generic shape.
pub trait AccountRepository: Repository<AccountId, Account> {}

// Blanket impl: anything that is a `Repository<AccountId, Account>` is an
// `AccountRepository`. Lets `InMemoryRepository<AccountId, Account>` satisfy the
// port with no boilerplate.
impl<R> AccountRepository for R where R: Repository<AccountId, Account> {}

/// Hashes a plaintext password. A port so the demo's stub hasher swaps for a
/// real argon2 adapter without touching the service.
pub trait PasswordHasher: Send + Sync {
    fn hash(&self, plaintext: &str) -> String;
}
