//! Accounts adapters — concrete `impl`s of this crate's ports.

use crate::ports::PasswordHasher;

/// A demo hasher (NOT cryptographic — a placeholder for an argon2 adapter behind
/// a feature gate). Isolated here so swapping to the real thing touches no
/// service code: the service only knows the `PasswordHasher` port.
pub struct DemoPasswordHasher;

impl PasswordHasher for DemoPasswordHasher {
    fn hash(&self, plaintext: &str) -> String {
        // Deterministic fold — stands in for a real KDF at the port boundary.
        let mut acc: u64 = 1469598103934665603;
        for b in plaintext.bytes() {
            acc ^= b as u64;
            acc = acc.wrapping_mul(1099511628211);
        }
        format!("fnv1a::{acc:016x}")
    }
}
