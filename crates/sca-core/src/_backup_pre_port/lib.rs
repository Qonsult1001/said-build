//! # SCA — Said Crystalline Attention
//!
//! Pluggable deterministic search engine. Works with ANY embedding model
//! or WITHOUT any embedding model (pure lexical mode).
//!
//! ## Architecture
//!
//! One engine: `CrystallineCore` — handles everything:
//! - ART (Adaptive Radix Tree) inverted index for O(1) token lookup
//! - 1-bit holographic quantization (16-view) for Hamming search
//! - IDF-weighted hybrid scoring with phrase matching + span density
//! - 3-path auto-routing: PureLexical / PureSemantic / FullHybrid
//! - Soundex + Levenshtein fuzzy matching for typo recovery
//! - Parallel scoring via rayon (zero per-doc allocations)
//!
//! ## Usage
//!
//! ```ignore
//! use sca_core::CrystallineCore;
//!
//! let mut engine = CrystallineCore::new();
//! engine.set_corpus_mean(mean_vec);
//! engine.load_idf_fast(words, scores);
//! engine.set_holographic_16view(true, None);
//! engine.add_docs_quantized(ids, embeddings_flat, passage_counts, gammas, doc_words);
//! let results = engine.search_unified_quantized(&query_emb, "query text", 10);
//! ```

pub mod crystalline;
pub mod engine;
pub mod latent_cluster;
pub mod state;
pub mod storage;

// Reference files (not compiled into the crate — for tracing the full LAM pipeline):
// - engine_pyo3.rs  — PyO3 bridge showing how Python calls CrystallineCore
// - model.rs        — LAM neural network (pluggable, any embedder replaces this)
// - storage_mmap.rs — Production mmap storage (upgrade path from storage.rs)
// - license.rs      — Licensing system (for commercial deployment)
// - secrets.rs      — Matryoshka embedding truncation

// Re-export the engine at crate root
pub use crystalline::CrystallineCore;
pub use crystalline::QueryRoute;
// Re-export the LEx integration wrapper
pub use engine::{ScaEngine, ScaHit};
// Re-export the latent space + cluster pre-filter + dual encoder
pub use latent_cluster::{LatentClusterIndex, LatentEntry, LatentSpace, DualEncoder, EncoderSource, EncodedEntry};
#[cfg(feature = "static-embed")]
pub use latent_cluster::StaticEncoder;
