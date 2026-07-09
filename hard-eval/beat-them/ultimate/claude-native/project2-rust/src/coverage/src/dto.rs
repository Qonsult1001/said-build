//! Coverage DTOs — projection that crosses the surface boundary.

use crate::model::Coverage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageDto {
    pub id: String,
    pub name: String,
    pub limit: u64,
    pub active: bool,
}

impl From<&Coverage> for CoverageDto {
    fn from(c: &Coverage) -> Self {
        Self {
            id: c.id().to_string(),
            name: c.name().to_string(),
            limit: c.limit(),
            active: c.is_active(),
        }
    }
}
