//! Per-role agent overlays. Each agent picks the shared sections it
//! needs and adds role-specific text on top.
//!
//! Today: `answerer` (the current main-loop agent). Tomorrow:
//! `searcher`, `writer`, `compiler` per the multi-agent architecture
//! (Anthropic's Explore / Plan / Verification model adapted for .said).

pub mod answerer;
