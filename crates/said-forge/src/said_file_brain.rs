//! `SaidFileBrain` — the production adapter wrapping `sca_core::SaidFile`.
//!
//! Implements both `BrainIo` (frame writes + tombstone + iteration) and
//! `BrainAccess` (pillar-scoped retrieval). Used by `said-cli`'s forge
//! subcommand + tests that need a real brain.

use crate::frame::BrainIo;
use crate::grounding::{BrainAccess, FrameMeta as GroundingFrameMeta};
use crate::story::Pillar as ForgePillar;
use crate::{ForgeError, ForgeResult};
use sca_core::frames::Pillar as CorePillar;
use sca_core::said_file::SaidFile;
use std::collections::HashSet;

pub struct SaidFileBrain<'a> {
    pub said: &'a mut SaidFile,
}

impl<'a> SaidFileBrain<'a> {
    pub fn new(said: &'a mut SaidFile) -> Self {
        Self { said }
    }
}

fn to_core_pillar(p: ForgePillar) -> CorePillar {
    match p {
        ForgePillar::Episodic => CorePillar::Episodic,
        ForgePillar::Semantic => CorePillar::Semantic,
        ForgePillar::Procedural => CorePillar::Procedural,
        ForgePillar::External => CorePillar::External,
        ForgePillar::Code => CorePillar::Code,
        ForgePillar::Memory => CorePillar::Memory,
        ForgePillar::Document => CorePillar::Document,
    }
}

fn from_core_pillar(p: CorePillar) -> ForgePillar {
    match p {
        CorePillar::Episodic => ForgePillar::Episodic,
        CorePillar::Semantic => ForgePillar::Semantic,
        CorePillar::Procedural => ForgePillar::Procedural,
        CorePillar::External => ForgePillar::External,
        CorePillar::Code => ForgePillar::Code,
        CorePillar::Memory => ForgePillar::Memory,
        CorePillar::Document => ForgePillar::Document,
    }
}

impl<'a> BrainIo for SaidFileBrain<'a> {
    fn remember(
        &mut self,
        title: &str,
        body: &str,
        pillar: ForgePillar,
        tags: Vec<String>,
    ) -> ForgeResult<String> {
        let core_pillar = to_core_pillar(pillar);
        let frame_id = self.said.remember_with_pillar(None, body, Some(title), core_pillar, tags);
        Ok(format!("frame#{}", frame_id))
    }

    fn find_body_by_tag(&mut self, tag: &str) -> Option<String> {
        // Two-phase: find the matching doc_id (immutable borrow), then
        // drop it and call SaidFile::get (which needs &mut self).
        //
        // `get_all_frames_with_pending` is intentional: put_with_pillar
        // appends to `pending` until compact/save flushes, so stories
        // written in the current session must be visible immediately.
        let doc_id = self
            .said
            .frames
            .get_all_frames_with_pending()
            .into_iter()
            .find(|m| m.tags.iter().any(|t| t == tag))
            .map(|m| m.doc_id.clone())?;
        self.said.get(&doc_id)
    }

    fn iter_tags(&self) -> Vec<(String, Vec<String>, i64)> {
        // Pending-aware: newly-written frames (not yet flushed by
        // compact/save) must show up to the runner + mcp helpers.
        self.said
            .frames
            .get_all_frames_with_pending()
            .into_iter()
            .map(|m| {
                (
                    m.doc_id.clone(),
                    m.tags.clone(),
                    m.created_at as i64,
                )
            })
            .collect()
    }

    fn delete(&mut self, doc_id: &str) -> bool {
        // iter_tags emits doc_ids. FrameStore's delete requires a frame_id,
        // so we look up the meta first.
        let fid = self.said.frames.get_meta(doc_id).map(|m| m.id);
        if let Some(fid) = fid {
            return self.said.mark_frame_deleted(fid);
        }
        false
    }

    fn save(&mut self) -> ForgeResult<()> {
        self.said.save().map_err(|e| ForgeError::Brain(format!("save: {}", e)))
    }
}

impl<'a> BrainAccess for SaidFileBrain<'a> {
    fn search_by_pillar(
        &mut self,
        query: &str,
        top_k: usize,
        pillars: &HashSet<ForgePillar>,
    ) -> Vec<(String, f32)> {
        let core_pillars: HashSet<CorePillar> = pillars.iter().map(|p| to_core_pillar(*p)).collect();
        let scope = if core_pillars.is_empty() { None } else { Some(&core_pillars) };
        self.said.search_by_pillar(query, top_k, scope)
    }

    fn sym(&mut self, name: &str, top_k: usize) -> Vec<(String, ForgePillar)> {
        let hits = self.said.sym(name, top_k);
        hits.into_iter()
            .map(|h| {
                let pillar = self
                    .said
                    .frames
                    .get_meta(&h.doc_id)
                    .map(|m| from_core_pillar(m.pillar))
                    .unwrap_or(ForgePillar::Episodic);
                (h.doc_id, pillar)
            })
            .collect()
    }

    fn meta(&self, frame_id: &str) -> Option<GroundingFrameMeta> {
        // frame_id is expected as the brain's `doc_id`.
        // SaidFile::get needs &mut self; BrainAccess::meta is &self.
        // We return metadata with an empty snippet — callers that need
        // the body use SaidFile::get(&doc_id) directly.
        let meta = self.said.frames.get_meta(frame_id)?;
        Some(GroundingFrameMeta {
            frame_id: frame_id.to_string(),
            tag: meta.tags.first().cloned().unwrap_or_default(),
            pillar: from_core_pillar(meta.pillar),
            snippet: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::Pillar;

    #[test]
    fn pillar_roundtrip_across_all_six() {
        for p in [
            Pillar::Episodic,
            Pillar::Semantic,
            Pillar::Procedural,
            Pillar::External,
            Pillar::Code,
            Pillar::Memory,
        ] {
            assert_eq!(p, from_core_pillar(to_core_pillar(p)));
        }
    }
}
