//! `accounts` capability crate (Ring 2): token login + userprofile.
//!
//! Depends inward on `sca-core` only. Owns the `Account` aggregate, the
//! `AccountRepository`/`PasswordHasher` ports, the `AccountService` use cases,
//! and the demo hasher adapter. Re-exports its deliberate public API here.

pub mod adapters;
pub mod api;
pub mod error;
pub mod login;
pub mod model;
pub mod ports;

pub use adapters::DemoPasswordHasher;
pub use api::AccountsApi;
pub use error::{AccountsError, AccountsResult};
pub use login::{
    AccountService, LoginRequest, RegisterRequest, TokenDto, UserProfileDto,
};
pub use model::Account;
pub use ports::{AccountRepository, PasswordHasher};
