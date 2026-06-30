//! Search ports — defined in this crate's domain.

use sca_core::{AccountId, LeadId, Repository};

use crate::model::{Lead, UserPreferences};

/// The lead repository port — named role over the kernel's generic `Repository`.
pub trait LeadRepository: Repository<LeadId, Lead> {}
impl<R> LeadRepository for R where R: Repository<LeadId, Lead> {}

/// The user-preferences repository port, keyed by the owner account id.
pub trait PreferencesRepository: Repository<AccountId, UserPreferences> {}
impl<R> PreferencesRepository for R where R: Repository<AccountId, UserPreferences> {}
