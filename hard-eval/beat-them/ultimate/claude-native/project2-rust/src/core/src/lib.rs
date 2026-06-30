//! Kernel crate (Ring 1).
//!
//! Holds the cross-cutting domain primitives every capability shares: identity,
//! authentication context, the generic `Repository` port, and the shared error
//! taxonomy. Target-agnostic — no I/O, no async runtime, WASM-safe. Capability
//! crates depend inward on this; nothing here depends on a capability or surface.

pub mod error;
pub mod identity;
pub mod ports;

pub use error::{CoreError, CoreResult};
pub use identity::{AuthContext, Token, UserId};
pub use ports::Repository;
