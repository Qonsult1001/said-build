//! The `coverages` use case — list a user's global coverages.
//!
//! Renders the same blueprint as Accounts: validate -> authorize -> query via
//! repository -> map to DTO -> return. Only the module-specific 20% (the owner
//! filter and the coverage DTO) differs.

use sca_core::{AccountId, CoverageId, IdGenerator};

use crate::error::{CoverageError, CoverageResult};
use crate::model::Coverage;
use crate::ports::CoverageRepository;

#[derive(Debug, Clone)]
pub struct CreateCoverageRequest {
    pub owner_id: String,
    pub region: String,
    pub limit_cents: u64,
}

/// Outbound DTO — never the aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageDto {
    pub id: String,
    pub owner_id: String,
    pub region: String,
    pub limit_cents: u64,
    pub active: bool,
}

impl CoverageDto {
    fn from_aggregate(c: &Coverage) -> Self {
        Self {
            id: c.id().as_str().to_string(),
            owner_id: c.owner().as_str().to_string(),
            region: c.region().to_string(),
            limit_cents: c.limit_cents(),
            active: c.is_active(),
        }
    }
}

pub struct CoverageService<'a> {
    repo: &'a dyn CoverageRepository,
    ids: &'a dyn IdGenerator,
}

impl<'a> CoverageService<'a> {
    pub fn new(repo: &'a dyn CoverageRepository, ids: &'a dyn IdGenerator) -> Self {
        Self { repo, ids }
    }

    pub fn create(&self, req: CreateCoverageRequest) -> CoverageResult<CoverageDto> {
        // 1. validate
        if req.owner_id.is_empty() {
            return Err(CoverageError::Validation("owner required".into()));
        }
        // 2. authorize (owner scope is the caller's account)
        let owner = AccountId::new(req.owner_id);
        // 3. persist via repository
        let id = CoverageId::new(self.ids.new_id());
        let coverage = Coverage::open(id.clone(), owner, req.region, req.limit_cents)?;
        self.repo.upsert(id, coverage.clone())?;
        // 4. map to DTO  5. return
        Ok(CoverageDto::from_aggregate(&coverage))
    }

    /// List the global coverages owned by one user.
    pub fn list_for_user(&self, owner_id: &str) -> CoverageResult<Vec<CoverageDto>> {
        // 1. validate
        if owner_id.is_empty() {
            return Err(CoverageError::Validation("owner required".into()));
        }
        // 2. authorize: scope to this owner  3. query repository
        let owner = AccountId::new(owner_id.to_string());
        let mut out: Vec<CoverageDto> = self
            .repo
            .list()?
            .iter()
            .filter(|c| c.owner() == &owner)
            .map(CoverageDto::from_aggregate)
            .collect();
        // stable order so the response is deterministic
        out.sort_by(|a, b| a.id.cmp(&b.id));
        // 4. (already mapped)  5. return
        Ok(out)
    }

    pub fn get(&self, coverage_id: &str) -> CoverageResult<CoverageDto> {
        let id = CoverageId::new(coverage_id.to_string());
        let coverage = self
            .repo
            .get(&id)?
            .ok_or_else(|| CoverageError::NotFound(coverage_id.to_string()))?;
        Ok(CoverageDto::from_aggregate(&coverage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sca_core::{InMemoryRepository, SequentialIdGenerator};

    #[test]
    fn list_scopes_to_owner() {
        let repo = InMemoryRepository::<CoverageId, Coverage>::new();
        let ids = SequentialIdGenerator::new("cov");
        let svc = CoverageService::new(&repo, &ids);

        svc.create(CreateCoverageRequest {
            owner_id: "u1".into(),
            region: "EU".into(),
            limit_cents: 5000,
        })
        .unwrap();
        svc.create(CreateCoverageRequest {
            owner_id: "u2".into(),
            region: "US".into(),
            limit_cents: 9000,
        })
        .unwrap();

        let mine = svc.list_for_user("u1").unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].region, "EU");
    }
}
