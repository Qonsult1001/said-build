//! Dev-Spec-driven op derivation. For each entity we test, look at
//! the Dev Spec markdown files (already parsed into `DevSpecEndpoint`
//! by `said-forge::dev_spec::parser::walk_dev_spec_dir`) and decide
//! which lifecycle ops Dev Spec **actually defines**. The harness then
//! runs ONLY those ops, and "green" means "every spec'd op passed."
//!
//! This replaces the prior assumption that every entity has all five
//! lifecycle steps. Some entities are POST-only; others are POST +
//! GET-by-id only; others have the full CRUD. The Dev Spec markdown
//! is the source of truth.

use crate::dev_spec::types::DevSpecEndpoint;
use std::collections::BTreeMap;

/// One lifecycle role the harness can attempt per entity. The harness
/// already speaks these (POST create → GET by id → PUT update →
/// GET re-read → GET list); now we make them spec-gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum LifecycleOp {
    PostCreate,
    GetById,
    PutUpdate,
    GetList,
}

impl LifecycleOp {
    pub fn label(&self) -> &'static str {
        match self {
            Self::PostCreate => "POST create",
            Self::GetById   => "GET by id",
            Self::PutUpdate => "PUT update",
            Self::GetList   => "GET list",
        }
    }
}

/// Set of Dev-Spec-defined lifecycle ops for one entity.
///
/// This is the GROUND TRUTH the harness uses to decide whether a
/// missing step is a "broken" outcome or a "not-spec'd" outcome that
/// should be silently skipped.
#[derive(Debug, Clone, Default)]
pub struct ExpectedOps {
    pub post_create: bool,
    pub get_by_id:   bool,
    pub put_update:  bool,
    pub get_list:    bool,
}

impl ExpectedOps {
    pub fn count(&self) -> usize {
        [self.post_create, self.get_by_id, self.put_update, self.get_list]
            .iter().filter(|b| **b).count()
    }
    pub fn has(&self, op: LifecycleOp) -> bool {
        match op {
            LifecycleOp::PostCreate => self.post_create,
            LifecycleOp::GetById   => self.get_by_id,
            LifecycleOp::PutUpdate => self.put_update,
            LifecycleOp::GetList   => self.get_list,
        }
    }
}

/// Build `entity → ExpectedOps` from the Dev Spec endpoint list.
///
/// Mapping rule (lifecycle role of an endpoint, by method + path shape):
///   - POST /<entity>            → PostCreate
///   - GET  /<entity>/{id}       → GetById
///   - PUT  /<entity>/{id}       → PutUpdate
///   - GET  /<entities>          → GetList
///   - everything else           → not a lifecycle op (sub-resource,
///     transitions, balance, etc.) → ignored here
///
/// Entity-name attribution: the first non-param segment of the path,
/// singularised, is the entity bucket. The build order's PascalCase
/// entity name resolves at lookup time via singular/plural matching.
pub fn build_expected_ops(
    endpoints: &[DevSpecEndpoint],
) -> BTreeMap<String, ExpectedOps> {
    let mut out: BTreeMap<String, ExpectedOps> = BTreeMap::new();
    for ep in endpoints {
        let segs: Vec<&str> = ep.path.split('/').filter(|s| !s.is_empty()).collect();
        if segs.is_empty() {
            continue;
        }
        let is_param = |s: &str| s.starts_with('{') && s.ends_with('}');
        let n_params = segs.iter().filter(|s| is_param(s)).count();
        let n_segs = segs.len();
        // First non-param segment, lowercased + singularised. Used as
        // the entity-bucket key. Build-order names resolve through
        // singular/plural matching at run time (see below).
        let entity_seg = segs.iter().find(|s| !is_param(s));
        let entity_seg = match entity_seg { Some(s) => *s, None => continue };
        let entity_key = crate::dev_spec::parser::singularise(&entity_seg.to_lowercase());

        let role: Option<LifecycleOp> = match (ep.method.as_str(), n_segs, n_params) {
            ("POST", 1, 0) => Some(LifecycleOp::PostCreate),
            ("PUT",  2, 1) => Some(LifecycleOp::PutUpdate),
            ("GET",  2, 1) => Some(LifecycleOp::GetById),
            ("GET",  1, 0) => Some(LifecycleOp::GetList),
            _ => None,
        };

        if let Some(r) = role {
            let entry = out.entry(entity_key).or_default();
            match r {
                LifecycleOp::PostCreate => entry.post_create = true,
                LifecycleOp::GetById   => entry.get_by_id   = true,
                LifecycleOp::PutUpdate => entry.put_update  = true,
                LifecycleOp::GetList   => entry.get_list    = true,
            }
        }
    }
    out
}

/// Look up an entity's ExpectedOps. Build-order names are PascalCase
/// (`ProgramManager`); Dev Spec entity-keys are lowercased+singularised
/// (`programmanager`). Resolve via singularise of the lowercased name.
pub fn for_entity<'a>(
    map: &'a BTreeMap<String, ExpectedOps>,
    entity: &str,
) -> Option<&'a ExpectedOps> {
    let key = crate::dev_spec::parser::singularise(&entity.to_lowercase());
    map.get(&key)
}
