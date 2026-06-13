//! Editor adapters — write native skill files for AI editors.
//!
//! MVP ships `ClaudeAdapter` only (spec §11.1). Cursor / Copilot are named
//! follow-ups per spec §11.2.

pub mod claude;

use crate::generator::GenerationResult;
use crate::{ForgeResult, Story};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectedStory<'a> {
    pub story: &'a Story,
    pub result: &'a GenerationResult,
}

pub trait EditorAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn skill_path(&self, project_root: &Path, slug: &str) -> PathBuf;
    fn write_skill(&self, project_root: &Path, proj: &ProjectedStory<'_>) -> ForgeResult<PathBuf>;
    fn remove_skill(&self, project_root: &Path, slug: &str) -> ForgeResult<bool>;
}
