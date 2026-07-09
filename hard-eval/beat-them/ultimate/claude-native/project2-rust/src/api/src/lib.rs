//! API surface (Ring 3).
//!
//! ONE outer interface exposing the four capabilities. Holds NO business logic:
//! each handler parses its input, delegates to a capability service, and returns
//! the DTO (or an `anyhow::Error` mapped from the typed capability error). Surface
//! crates use `anyhow`, never `thiserror`.

pub mod handlers;

pub use handlers::AppState;
