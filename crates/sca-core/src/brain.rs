//! Brain layer for SCA .said files — the memory learns from usage.
//!
//! Three capabilities on top of the base SCRM retrieval index:
//!
//! 1. QUERY LOG — append-only record of searches + which doc was used
//!    Tracks: query text hash, top result doc_id, timestamp, score
//!
//! 2. RECONSOLIDATION — documents that get recalled frequently get boosted
//!    Each doc has a recall_weight that increases on access and decays over time
//!    Recall weight is factored into hybrid scoring as a multiplier
//!
//! 3. CONSOLIDATION — recompute corpus statistics when significant changes occur
//!    Triggered when recall patterns shift (new hot docs, cold docs)
//!
//! The .said file literally gets smarter with use. Not by parameter tuning —
//! by learning which documents matter from actual query patterns.

use std::collections::HashMap;

/// A single query log entry.
#[derive(Debug, Clone)]
pub struct QueryEntry {
    /// Hash of query text (u64, not the full text — saves space)
    pub query_hash: u64,
    /// Doc ID of top result that was returned
    pub top_doc_id: String,
    /// Score of top result
    pub score: f32,
    /// Unix timestamp (seconds)
    pub timestamp: u64,
}

/// Per-document recall state for reconsolidation.
#[derive(Debug, Clone)]
pub struct DocRecallState {
    /// How many times this doc has been returned as a top result
    pub recall_count: u32,
    /// Current recall weight (1.0 = baseline, higher = boosted)
    pub recall_weight: f32,
    /// Last time this doc was recalled (unix timestamp)
    pub last_recalled: u64,
}

/// The brain layer — lives alongside the SCRM index in the .said file.
///
/// Combines three systems from the research:
/// 1. Document-level reconsolidation (recall_weight per doc)
/// 2. S_slow tensor (64×64 semantic relationship matrix from memory_as_a_service.py)
/// 3. Cross-timescale drift (corpus_mean adapts to query patterns)
pub struct Brain {
    /// Append-only query log (last N entries, ring buffer)
    pub query_log: Vec<QueryEntry>,
    /// Per-document recall state
    pub doc_recall: HashMap<String, DocRecallState>,
    /// Maximum query log entries to keep
    pub max_log_entries: usize,
    /// Reconsolidation boost per recall (additive, clamped)
    pub reconsolidation_boost: f32,
    /// Decay rate per consolidation cycle (multiplicative)
    pub decay_rate: f32,
    /// Number of consolidation cycles run
    pub consolidation_cycles: u32,
    /// Running sum of query embeddings (for cross-timescale mean drift)
    pub(crate) query_emb_sum: Vec<f64>,
    /// Number of query embeddings accumulated
    pub(crate) query_emb_count: u64,
    /// Cross-timescale influence rate
    pub cross_influence: f32,
    /// Burst detection: last N recalled doc_ids
    recent_recalls: Vec<String>,

    // =========================================================================
    // S_SLOW TENSOR — from memory_as_a_service.py / discoveries.md
    // The semantic relationship matrix: S_slow = S_slow * 0.999 + K^T @ U
    // Accumulates meaning from every remember/recall interaction.
    // Magnitude encodes recency (discovery #001).
    // =========================================================================

    /// S_slow: 64×64 semantic memory tensor (row-major, d_k × d_v).
    /// Accumulates K^T @ U from every memory write.
    /// Cross-document synthesis happens here — two documents about
    /// "photonic inverters" connect through the shared embedding signal.
    pub s_slow: Vec<f32>,
    /// Dimension of S_slow (d_k = d_v = 64 for said-lam-static)
    pub s_slow_dim: usize,
    /// S_slow decay rate per consolidation (0.999 from formula)
    pub s_slow_decay: f32,
}

impl Brain {
    pub fn new() -> Self {
        Self {
            query_log: Vec::new(),
            doc_recall: HashMap::new(),
            max_log_entries: 10000,
            reconsolidation_boost: 0.05,
            decay_rate: 0.95,
            consolidation_cycles: 0,
            query_emb_sum: Vec::new(),
            query_emb_count: 0,
            cross_influence: 0.05, // matches S_slow += 0.05 * S_fast from formula
            recent_recalls: Vec::new(),
            s_slow: vec![0.0f32; 64 * 64], // 64×64 semantic tensor (16KB)
            s_slow_dim: 64,
            s_slow_decay: 0.999, // from memory_as_a_service.py
        }
    }

    // =========================================================================
    // QUERY LOGGING
    // =========================================================================

    /// Log a search result. Called after every search.
    pub fn log_query(&mut self, query: &str, top_doc_id: &str, score: f32) {
        let query_hash = Self::hash_query(query);
        let timestamp = crate::time_compat::unix_secs();

        self.query_log.push(QueryEntry {
            query_hash,
            top_doc_id: top_doc_id.to_string(),
            score,
            timestamp,
        });

        // Ring buffer: keep last N entries
        if self.query_log.len() > self.max_log_entries {
            self.query_log.remove(0);
        }

        // Reconsolidate the recalled document
        self.reconsolidate(top_doc_id, timestamp);
    }

    /// Simple hash for query text (FNV-1a style, fast and deterministic)
    fn hash_query(query: &str) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in query.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }

    // =========================================================================
    // RECONSOLIDATION — recalled documents get stronger
    // =========================================================================

    /// Boost a document's recall weight when it's returned as a search result.
    /// Uses burst detection: 3+ recalls in last 10 queries → extra bonus.
    fn reconsolidate(&mut self, doc_id: &str, timestamp: u64) {
        self.track_recall(doc_id);

        let is_burst = self.detect_burst(doc_id);

        let state = self.doc_recall.entry(doc_id.to_string()).or_insert(DocRecallState {
            recall_count: 0,
            recall_weight: 1.0,
            last_recalled: 0,
        });

        state.recall_count += 1;
        state.last_recalled = timestamp;

        // Diminishing returns + burst bonus (from dual-memory formula)
        let base_boost = self.reconsolidation_boost / (state.recall_count as f32).sqrt();
        let burst_bonus = if is_burst { 0.15 } else { 0.0 };
        state.recall_weight = (state.recall_weight + base_boost + burst_bonus).min(2.0);
    }

    /// Get recall weight for a document (1.0 = neutral, >1.0 = boosted).
    /// Includes recency factor from dual-memory formula:
    ///   score = base * 0.8 + base * recency * 0.2
    /// where recency = 1.0 (just recalled) -> 0.0 (recalled long ago)
    pub fn get_recall_weight(&self, doc_id: &str) -> f32 {
        match self.doc_recall.get(doc_id) {
            Some(state) => {
                let now = crate::time_compat::unix_secs();
                // Recency: 1.0 at recall time, decays to 0.0 over 24 hours
                let age_secs = now.saturating_sub(state.last_recalled) as f32;
                let recency = (-age_secs / 86400.0).exp(); // e^(-t/24h)
                // Recency bonus: adds up to 20% on top of recall_weight for recent recalls
                // Never reduces below base recall_weight. Clamp to the documented [1.0, 2.0] range:
                // the base weight is already capped at 2.0, but the recency multiplier (×1.2) could
                // push the effective value to 2.4 — the docs (3.3-brain, public-overview) promise a
                // 1.0–2.0 weight, and downstream Layer-9 scaling assumes that ceiling.
                (state.recall_weight * (1.0 + 0.2 * recency)).min(2.0)
            }
            None => 1.0,
        }
    }

    // =========================================================================
    // CONSOLIDATION — periodic maintenance, decay cold memories
    // =========================================================================

    /// Run one consolidation cycle. Call periodically (e.g., every N queries or on idle).
    /// Decays recall weights for documents not recently accessed.
    /// Returns number of documents whose weights changed.
    pub fn consolidate(&mut self) -> usize {
        let now = crate::time_compat::unix_secs();

        let mut changed = 0;
        let mut to_remove = Vec::new();

        for (doc_id, state) in self.doc_recall.iter_mut() {
            // Only decay if not recalled recently (>1 hour ago)
            let age_hours = (now.saturating_sub(state.last_recalled)) / 3600;
            if age_hours > 1 {
                let old_weight = state.recall_weight;
                state.recall_weight *= self.decay_rate;

                // If weight decayed back to ~1.0, remove tracking (save memory)
                if state.recall_weight < 1.01 {
                    state.recall_weight = 1.0;
                    if state.recall_count == 0 {
                        to_remove.push(doc_id.clone());
                    }
                }

                if (state.recall_weight - old_weight).abs() > 0.001 {
                    changed += 1;
                }
            }
        }

        for doc_id in to_remove {
            self.doc_recall.remove(&doc_id);
        }

        self.consolidation_cycles += 1;
        changed
    }

    // =========================================================================
    // S_SLOW TENSOR — semantic relationship matrix from memory_as_a_service.py
    // Formula: S_slow = S_slow * decay + K^T @ U
    // Discovery #001: magnitude encodes recency naturally
    // =========================================================================

    /// Write to S_slow: accumulate the semantic relationship K^T @ U.
    /// Called on every remember() with the document's 64-dim embedding.
    /// The embedding serves as both K (key) and U (value).
    pub fn s_slow_write(&mut self, embedding: &[f32]) {
        // Adapt S_slow to the ACTUAL encoder dim on first/changed write. Brain::new defaults to
        // 64×64 (said-lam-static 2M), but the 4M model emits 128-dim embeddings — without this
        // the dim guard below rejected EVERY write and s_slow stayed all-zeros (dream
        // consolidation silently dead with the 4M encoder). Re-init to dim×dim when needed.
        if !embedding.is_empty() && self.s_slow_dim != embedding.len() {
            self.s_slow_dim = embedding.len();
            self.s_slow = vec![0.0f32; embedding.len() * embedding.len()];
        }
        let dim = self.s_slow_dim;
        if embedding.len() != dim || self.s_slow.len() != dim * dim { return; }

        // S_slow = S_slow * decay + K^T @ U
        // K and U are both the embedding (self-associative)
        for i in 0..dim {
            for j in 0..dim {
                self.s_slow[i * dim + j] =
                    self.s_slow[i * dim + j] * self.s_slow_decay
                    + embedding[i] * embedding[j];
            }
        }
    }

    /// Read from S_slow: compute Q @ S_slow to find semantic connections.
    /// Returns a score indicating how strongly the query connects to stored knowledge.
    /// This is how cross-document synthesis works — the tensor links
    /// "Eleanor's photonic inverters" to "Aura Systems licensing."
    pub fn s_slow_read(&self, query_embedding: &[f32]) -> f32 {
        let dim = self.s_slow_dim;
        if query_embedding.len() != dim || self.s_slow.len() != dim * dim { return 0.0; }

        // output = Q @ S_slow → 64-dim vector
        // score = ||output|| (magnitude indicates connection strength)
        let mut output = vec![0.0f32; dim];
        for i in 0..dim {
            for j in 0..dim {
                output[i] += query_embedding[j] * self.s_slow[j * dim + i];
            }
        }

        // Magnitude = connection strength (discovery #001: also encodes recency)
        let magnitude: f32 = output.iter().map(|v| v * v).sum::<f32>().sqrt();
        magnitude
    }

    /// S_slow magnitude — natural timestamp (discovery #001).
    /// Higher = more knowledge accumulated = more recent/active.
    pub fn s_slow_magnitude(&self) -> f32 {
        self.s_slow.iter().map(|v| v * v).sum::<f32>().sqrt()
    }

    // =========================================================================
    // CROSS-TIMESCALE LEARNING — corpus_mean drifts toward query distribution
    // =========================================================================

    /// Accumulate a query embedding for cross-timescale mean drift.
    /// Called after every search with the query embedding.
    pub fn accumulate_query_embedding(&mut self, query_emb: &[f32]) {
        if self.query_emb_sum.is_empty() {
            self.query_emb_sum = vec![0.0f64; query_emb.len()];
        }
        for (i, &v) in query_emb.iter().enumerate() {
            if i < self.query_emb_sum.len() {
                self.query_emb_sum[i] += v as f64;
            }
        }
        self.query_emb_count += 1;
    }

    /// DREAM: the brain's offline consolidation pass.
    ///
    /// Inspired by the dual-memory formula:
    ///   S_slow += cross_influence * S_fast
    ///
    /// Applied to embeddings:
    ///   corpus_mean += cross_influence * (query_mean - corpus_mean)
    ///   corpus_std  += cross_influence * (query_std  - corpus_std)
    ///
    /// This shifts the binarization threshold toward the query distribution,
    /// so fingerprints become optimized for the actual questions being asked.
    ///
    /// Returns (new_corpus_mean, new_corpus_std) if enough queries accumulated.
    pub fn dream(
        &mut self,
        corpus_mean: &[f32],
        corpus_std: &[f32],
        min_queries: u64,
    ) -> Option<(Vec<f32>, Vec<f32>)> {
        if self.query_emb_count < min_queries || self.query_emb_sum.is_empty() {
            return None; // not enough signal yet
        }

        let dim = corpus_mean.len();
        if self.query_emb_sum.len() != dim {
            return None;
        }

        let n = self.query_emb_count as f64;
        let alpha = self.cross_influence as f64;

        // Compute query centroid
        let query_mean: Vec<f32> = self.query_emb_sum.iter()
            .map(|&s| (s / n) as f32)
            .collect();

        // Drift corpus_mean toward query_mean
        // new_mean = corpus_mean + alpha * (query_mean - corpus_mean)
        let new_mean: Vec<f32> = corpus_mean.iter().zip(query_mean.iter())
            .map(|(&cm, &qm)| cm + (alpha as f32) * (qm - cm))
            .collect();

        // Compute query variance per dimension
        // We don't have per-query embeddings stored, but we can estimate std
        // from the difference between query_mean and corpus_mean
        // For now: drift corpus_std toward a blend of current std and query deviation
        let new_std: Vec<f32> = corpus_std.iter().zip(corpus_mean.iter()).zip(query_mean.iter())
            .map(|((&cs, &cm), &qm)| {
                let deviation = (qm - cm).abs();
                // If queries deviate a lot from corpus mean in this dimension,
                // increase std (widen the decision boundary)
                let target_std = cs.max(deviation * 0.5);
                cs + (alpha as f32) * (target_std - cs)
            })
            .collect();

        // Reset accumulator for next dream cycle
        self.query_emb_sum = vec![0.0f64; dim];
        self.query_emb_count = 0;
        self.consolidation_cycles += 1;

        Some((new_mean, new_std))
    }

    // =========================================================================
    // BURST DETECTION — 3+ recalls of same doc in last 10 queries
    // =========================================================================

    /// Check if a doc_id has burst pattern (recalled 3+ times in last 10).
    fn detect_burst(&self, doc_id: &str) -> bool {
        self.recent_recalls.iter().filter(|d| d.as_str() == doc_id).count() >= 3
    }

    /// Track a recall for burst detection.
    fn track_recall(&mut self, doc_id: &str) {
        self.recent_recalls.push(doc_id.to_string());
        if self.recent_recalls.len() > 10 {
            self.recent_recalls.remove(0);
        }
    }

    // =========================================================================
    // DIAGNOSTICS
    // =========================================================================

    /// Get brain stats for monitoring.
    pub fn stats(&self) -> BrainStats {
        let total_recalls: u32 = self.doc_recall.values().map(|s| s.recall_count).fold(0u32, u32::saturating_add);
        let boosted_docs = self.doc_recall.values().filter(|s| s.recall_weight > 1.01).count();
        let max_weight = self.doc_recall.values()
            .map(|s| s.recall_weight)
            .fold(1.0f32, f32::max);

        BrainStats {
            query_log_size: self.query_log.len(),
            tracked_docs: self.doc_recall.len(),
            total_recalls,
            boosted_docs,
            max_recall_weight: max_weight,
            consolidation_cycles: self.consolidation_cycles,
            s_slow_magnitude: self.s_slow_magnitude(),
        }
    }

    // =========================================================================
    // SERIALIZATION — stored in .said file as BRAIN section
    // =========================================================================

    /// Serialize brain state to bytes.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"BRAN"); // magic

        // Query log (last 1000 entries max for serialization)
        let log_start = if self.query_log.len() > 1000 {
            self.query_log.len() - 1000
        } else { 0 };
        let log_slice = &self.query_log[log_start..];

        buf.extend_from_slice(&(log_slice.len() as u32).to_le_bytes());
        for entry in log_slice {
            buf.extend_from_slice(&entry.query_hash.to_le_bytes());
            let doc_bytes = entry.top_doc_id.as_bytes();
            buf.extend_from_slice(&(doc_bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(doc_bytes);
            buf.extend_from_slice(&entry.score.to_le_bytes());
            buf.extend_from_slice(&entry.timestamp.to_le_bytes());
        }

        // Doc recall state
        buf.extend_from_slice(&(self.doc_recall.len() as u32).to_le_bytes());
        for (doc_id, state) in &self.doc_recall {
            let doc_bytes = doc_id.as_bytes();
            buf.extend_from_slice(&(doc_bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(doc_bytes);
            buf.extend_from_slice(&state.recall_count.to_le_bytes());
            buf.extend_from_slice(&state.recall_weight.to_le_bytes());
            buf.extend_from_slice(&state.last_recalled.to_le_bytes());
        }

        // Metadata
        buf.extend_from_slice(&self.consolidation_cycles.to_le_bytes());

        // Cross-timescale accumulator (persists across sessions for dreaming)
        buf.extend_from_slice(&self.query_emb_count.to_le_bytes());
        buf.extend_from_slice(&(self.query_emb_sum.len() as u32).to_le_bytes());
        for &v in &self.query_emb_sum {
            buf.extend_from_slice(&v.to_le_bytes());
        }

        // S_slow tensor (64×64 = 16KB semantic relationship matrix)
        buf.extend_from_slice(b"SLOW"); // magic marker
        buf.extend_from_slice(&(self.s_slow_dim as u32).to_le_bytes());
        for &v in &self.s_slow {
            buf.extend_from_slice(&v.to_le_bytes());
        }

        buf
    }

    /// Deserialize brain state from BRAIN section bytes.
    ///
    /// IMPORTANT: caller MUST bound `data` to where BRAN actually ends in the
    /// file. If BRAN is followed by TRGM/SYMS/TOC bytes, feeding the tail of
    /// the file here causes those bytes to be parsed as millions of fake
    /// "query log entries" and billions of fake "dream cycles". That junk
    /// is then serialized back on the next save, doubling the file size
    /// every cycle. Always slice `data` to `[bran_start..next_section_start]`.
    pub fn deserialize(data: &[u8]) -> Result<Self, String> {
        if data.len() < 8 || &data[0..4] != b"BRAN" {
            return Err("Invalid BRAIN section".into());
        }

        let mut brain = Brain::new();
        let mut pos = 4;

        // Query log
        if pos + 4 > data.len() { return Ok(brain); }
        let n_log_raw = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;

        // Sanity guard: the query log is capped at `max_log_entries` (10,000
        // by default) in serialize. A value larger than that — or larger than
        // what could physically fit in `data` — means BRAN was corrupted or
        // over-read. Abort cleanly instead of reading garbage into memory.
        //
        // Worst case a single log entry is: 8 (hash) + 2 (doc_len) + 65535
        // (doc) + 4 (score) + 8 (ts) = 65557 bytes. But typical is ~60 bytes.
        // Upper-bound n_log by data size / 20 (minimum-size entry) to avoid
        // allocations for impossible counts.
        let max_plausible_log = data.len() / 20 + 1;
        let n_log = if n_log_raw > max_plausible_log.max(1_000_000) {
            return Ok(brain);  // corrupted — stop, return empty brain
        } else {
            n_log_raw.min(max_plausible_log)
        };

        for _ in 0..n_log {
            if pos + 8 > data.len() { break; }
            let query_hash = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
            pos += 8;

            if pos + 2 > data.len() { break; }
            let doc_len = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
            pos += 2;

            if pos + doc_len > data.len() { break; }
            let top_doc_id = String::from_utf8_lossy(&data[pos..pos+doc_len]).to_string();
            pos += doc_len;

            if pos + 12 > data.len() { break; }
            let score = f32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
            pos += 4;
            let timestamp = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
            pos += 8;

            brain.query_log.push(QueryEntry { query_hash, top_doc_id, score, timestamp });
        }

        // Doc recall state
        if pos + 4 > data.len() { return Ok(brain); }
        let n_docs_raw = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;

        // Same sanity guard as query_log — refuse to allocate for a plainly
        // impossible count. Minimum entry: 2 + 0 (doc) + 4 + 4 + 8 = 18 bytes.
        let max_plausible_docs = data.len() / 18 + 1;
        let n_docs = if n_docs_raw > max_plausible_docs.max(1_000_000) {
            return Ok(brain);
        } else {
            n_docs_raw.min(max_plausible_docs)
        };

        for _ in 0..n_docs {
            if pos + 2 > data.len() { break; }
            let doc_len = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
            pos += 2;

            if pos + doc_len > data.len() { break; }
            let doc_id = String::from_utf8_lossy(&data[pos..pos+doc_len]).to_string();
            pos += doc_len;

            if pos + 16 > data.len() { break; }
            let recall_count = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
            pos += 4;
            let recall_weight = f32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
            pos += 4;
            let last_recalled = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
            pos += 8;

            brain.doc_recall.insert(doc_id, DocRecallState {
                recall_count, recall_weight, last_recalled,
            });
        }

        // Metadata
        if pos + 4 <= data.len() {
            brain.consolidation_cycles = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
            pos += 4;
        }

        // Cross-timescale accumulator (optional, backward compatible)
        if pos + 12 <= data.len() {
            brain.query_emb_count = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
            pos += 8;
            let emb_dim = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
            pos += 4;
            brain.query_emb_sum.clear();
            for _ in 0..emb_dim {
                if pos + 8 > data.len() { break; }
                brain.query_emb_sum.push(f64::from_le_bytes(data[pos..pos+8].try_into().unwrap()));
                pos += 8;
            }
        }

        // S_slow tensor (optional, backward compatible — look for "SLOW" magic)
        if pos + 8 <= data.len() && &data[pos..pos+4] == b"SLOW" {
            pos += 4;
            let dim = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
            pos += 4;
            brain.s_slow_dim = dim;
            brain.s_slow = vec![0.0f32; dim * dim];
            for i in 0..dim * dim {
                if pos + 4 > data.len() { break; }
                brain.s_slow[i] = f32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
                pos += 4;
            }
        }

        Ok(brain)
    }
}

/// Brain diagnostics.
#[derive(Debug)]
pub struct BrainStats {
    pub query_log_size: usize,
    pub tracked_docs: usize,
    pub total_recalls: u32,
    pub boosted_docs: usize,
    pub max_recall_weight: f32,
    pub consolidation_cycles: u32,
    pub s_slow_magnitude: f32,
}
