//! said-forge — spec-driven workspace generator for .said.
//!
//! Turns a grounded .said brain plus a directive (OpenAPI / Markdown) into:
//! a specification, blueprint, plan, task list, and native Claude Code
//! skill file — one per user story. All artifacts stored as .said frames
//! with `forge:<type>:<hash>:<slug>` tags.
//!
//! See `docs/superpowers/specs/2026-04-22-said-forge-design.md` for the
//! authoritative design.

pub mod adapter;
pub mod business_config;
pub mod config;
pub mod directive;
pub mod error;
pub mod filter;
pub mod frame;
pub mod gap_report;
pub mod gap_structured;
pub mod gap_v2;
pub mod generator;
pub mod grounding;
pub mod schema;
pub mod schema_diff;
pub mod sql_catalog;
pub mod tech_grounding;
pub mod init;
pub mod llm;
pub mod mapping_service;
pub mod comparison;
pub mod openapi_standard;
pub mod plan_cli;
pub mod proc_analysis;
pub mod fitter;
pub mod dev_spec;
pub mod sandbox;
pub mod sql_to_openapi;
pub mod sql_verify;
#[cfg(feature = "forge-sql-verify")]
pub mod test_harness;
#[cfg(feature = "forge-sql-verify")]
pub mod coverage;
pub mod tester;
pub mod plan_scan;
pub mod sync;
pub mod workspace_config;
pub mod mcp_api;
pub mod projection;
pub mod runner;
pub mod said_file_brain;
pub mod skill_regen;
pub mod source;
pub mod story;
pub mod story_doc;
pub mod story_gen;
pub mod viz;
pub mod tag;

pub use config::{CostsConfig, ForgeConfig, GroundingConfig, LlmConfig, LlmProviderKind, MarkdownConfig};
pub use dev_spec::{
    BorrowDecision, Column, DevSpecEndpoint, DevSpecParam, DevSpecSchema, Entity, Erd,
    ForeignKey,
};
pub use error::{ForgeError, ForgeResult};
pub use filter::{matches as filter_matches, parse as filter_parse, Filter};
pub use generator::{
    generate, schema::output_schema, BrainRef, GeneratedArtifacts, GenerationResult, PlanDoc,
    PlanStep, SpecDoc, TaskItem,
};
pub use grounding::{
    apply_boost, retrieve, BrainAccess, FrameMeta as GroundingFrameMeta, GroundingHit,
    GroundingReport, QueryOutcome, RetrievalQuery,
};
pub use llm::{
    provider_from_config, CompletionRequest, CompletionResponse, LlmCapabilities, LlmFailureClass,
    LlmProvider, TokenUsage,
};
pub use llm::anthropic::AnthropicProvider;
pub use llm::openai_compat::OpenAICompatibleProvider;
pub use mapping_service::{
    Confidence, Glossary, MappingOverrides, MappingResult, MappingService, MappingTrace,
    TableRef as MappingTableRef, TableRole,
};
pub use openapi_standard::OpenApiStandard;
pub use adapter::{claude::ClaudeAdapter, EditorAdapter, ProjectedStory};
pub use mcp_api::{
    apply_token_cap, get_bundled, list_stories, status as story_status, StoryListItem,
    StoryStatusItem, FORGE_GET_TOKEN_CAP,
};
pub use said_file_brain::SaidFileBrain;
pub use skill_regen::{
    regenerate_contents, regenerate_skill, RegenOptions, RegenReport, RegeneratedEntry,
};
pub use business_config::{BusinessConfig, Stakeholder};
pub use story_doc::{generate_story_docs, StoryDocsReport, StoryMeta};
pub use story_gen::{generate_stories, StoriesReport};
pub use viz::{
    render_authority_flow, render_erd, render_op_dependency, render_viz_document, AuthorityPaths,
};
pub use frame::BrainIo;
pub use projection::{is_complete, remove_folder, write_folder, ProjectionMeta};
pub use runner::{
    classify_error, preflight_estimate, run_one, CircuitBreaker, RunOptions, StoryOutcome,
    StoryStatus,
};
pub use source::{DirectiveSource, SourceCapabilities, SourceRegistry};
pub use story::{DirectiveDoc, DirectiveMeta, Pillar, Story, StoryKind};
pub use tag::{forge_hash, forge_tag, parse_forge_tag, sanitize_slug, ForgeTagParts};
