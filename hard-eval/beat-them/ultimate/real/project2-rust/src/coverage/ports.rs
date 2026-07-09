//! Coverage ports — defined in this crate's domain.

use sca_core::{CoverageId, Repository};

use crate::model::Coverage;

/// The coverage repository port — a named role over the kernel's generic
/// `Repository`, so the in-memory adapter is reused and the service depends on
/// this role, not the generic shape.
pub trait CoverageRepository: Repository<CoverageId, Coverage> {}

impl<R> CoverageRepository for R where R: Repository<CoverageId, Coverage> {}
