//! Search adapters — the in-memory store aliases the surface wires.
//!
//! Both repositories reuse the kernel's `InMemoryRepository` via the blanket
//! port impls; this module documents the layer and homes future DB adapters.

use sca_core::{AccountId, InMemoryRepository, LeadId};

use crate::model::{Lead, UserPreferences};

pub type InMemoryLeadRepo = InMemoryRepository<LeadId, Lead>;
pub type InMemoryPreferencesRepo = InMemoryRepository<AccountId, UserPreferences>;
