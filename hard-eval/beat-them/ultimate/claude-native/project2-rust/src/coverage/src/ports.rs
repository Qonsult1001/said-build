//! Coverage ports.
//!
//! Reuses the generic kernel `Repository` port: coverages are stored per user,
//! so the port is `Repository<UserId, Vec<Coverage>>`. Defining a domain-named
//! alias keeps call sites readable while pointing at the kernel trait (no new
//! trait needed — the slice pattern collaborates through the lower crate's port).

use crate::model::Coverage;
use core::{Repository, UserId};

/// Domain-named persistence port for coverages, backed by the kernel trait.
pub trait CoverageRepository: Repository<UserId, Vec<Coverage>> {}

/// Blanket impl: any kernel `Repository` of the right shape is a `CoverageRepository`.
impl<T> CoverageRepository for T where T: Repository<UserId, Vec<Coverage>> {}
