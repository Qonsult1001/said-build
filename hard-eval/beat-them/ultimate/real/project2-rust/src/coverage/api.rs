//! Coverage API handler — thin binding over the service.

use sca_core::IdGenerator;

use crate::coverages::{CoverageDto, CoverageService, CreateCoverageRequest};
use crate::error::CoverageResult;
use crate::ports::CoverageRepository;

pub struct CoverageApi<'a> {
    service: CoverageService<'a>,
}

impl<'a> CoverageApi<'a> {
    pub fn new(repo: &'a dyn CoverageRepository, ids: &'a dyn IdGenerator) -> Self {
        Self {
            service: CoverageService::new(repo, ids),
        }
    }

    pub fn create(&self, req: CreateCoverageRequest) -> CoverageResult<CoverageDto> {
        self.service.create(req)
    }

    pub fn list_for_user(&self, owner_id: &str) -> CoverageResult<Vec<CoverageDto>> {
        self.service.list_for_user(owner_id)
    }

    pub fn get(&self, coverage_id: &str) -> CoverageResult<CoverageDto> {
        self.service.get(coverage_id)
    }
}
