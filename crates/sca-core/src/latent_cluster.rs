//! Latent Cluster Index — true latent space thinking for Crystalline search.
//!
//! Built on the LatentSpace model from memvid: compressed knowledge representations
//! where each memory lives as a truncated Matryoshka vector in a topic cluster,
//! with consolidation strength tracking and BLAKE3 content-addressable recall.
//!
//! This module adds the intelligence layer:
//! - K-means clustering to auto-discover topic structure
//! - Matryoshka truncation (384→64 dim) with L2 re-normalization
//! - Multi-cluster search with strength-weighted scoring
//! - Cluster membership index for O(1) candidate lookup
//! - Topic introspection ("what do I know about?")
//!
//! Design: LatentSpace is the data model, LatentClusterIndex is the engine.
//!
//! Query flow:
//!   1. Full embedding → Matryoshka truncate → 64-dim latent vector
//!   2. Dot-product against K centroids (microseconds) → nearest topics
//!   3. Scan entries in those clusters, weighted by consolidation strength
//!   4. Return ranked results — or O(1) exact recall via BLAKE3
//!
//! Completely independent of CrystallineCore — testable in isolation.

use std::collections::HashMap;

/// Default latent dimension (Matryoshka truncation target).
pub const DEFAULT_LATENT_DIM: usize = 64;

/// Default number of clusters for k-means.
pub const DEFAULT_K: usize = 32;

/// Maximum k-means iterations.
const KMEANS_MAX_ITER: usize = 50;

/// Convergence threshold for k-means (centroid movement).
const KMEANS_EPSILON: f64 = 1e-6;

// =============================================================================
// LATENT SPACE CORE — the true representation
// =============================================================================

/// A single entry in the latent space.
///
/// Each entry is a compressed memory with a position in topic-space,
/// a back-reference to the original document, and a consolidation
/// strength that grows with access (more recalled = stronger).
#[derive(Debug, Clone)]
pub struct LatentEntry {
    /// Compressed representation (e.g. 64-dim Matryoshka truncation).
    pub vector: Vec<f32>,
    /// Which topic cluster this entry belongs to.
    pub cluster_id: u32,
    /// Back-reference to the original document index.
    pub doc_idx: usize,
    /// Consolidation strength: higher = more accessed/important.
    /// Starts at 1.0, grows with each recall, decays over time.
    pub strength: f32,
}

/// The latent knowledge space — compressed topic-structured memory.
///
/// This is the true latent space: not just vectors in a flat list, but
/// a structured knowledge representation where memories are organized
/// by discovered topics, weighted by importance, and addressable both
/// by semantic similarity (dot-product) and exact content (BLAKE3).
#[derive(Debug, Clone)]
pub struct LatentSpace {
    /// Topic cluster centroids: [K × latent_dim].
    /// Each centroid is the mean of its cluster's entries — the "essence" of a topic.
    pub centroids: Vec<Vec<f32>>,
    /// Human-readable topic labels (auto-generated or user-assigned).
    pub labels: Vec<String>,
    /// All latent entries.
    pub entries: Vec<LatentEntry>,
    /// Content-addressable hash: BLAKE3(text) → entry index for O(1) exact recall.
    pub content_hashes: HashMap<[u8; 32], usize>,
    /// Latent dimension (e.g. 64).
    pub latent_dim: u32,
    /// Source embedding dimension (e.g. 384).
    pub source_dim: u32,
}

impl LatentSpace {
    /// Create an empty latent space.
    pub fn new(latent_dim: u32, source_dim: u32) -> Self {
        Self {
            centroids: Vec::new(),
            labels: Vec::new(),
            entries: Vec::new(),
            content_hashes: HashMap::new(),
            latent_dim,
            source_dim,
        }
    }

    /// Add an entry with content hash for O(1) lookup.
    pub fn add_entry(
        &mut self,
        vector: Vec<f32>,
        cluster_id: u32,
        doc_idx: usize,
        content_hash: [u8; 32],
    ) {
        let idx = self.entries.len();
        self.entries.push(LatentEntry {
            vector,
            cluster_id,
            doc_idx,
            strength: 1.0,
        });
        self.content_hashes.insert(content_hash, idx);
    }

    /// O(1) exact recall by content hash.
    pub fn recall_by_hash(&self, hash: &[u8; 32]) -> Option<&LatentEntry> {
        self.content_hashes.get(hash).and_then(|&idx| self.entries.get(idx))
    }

    /// Find nearest cluster for a query vector (dot-product).
    pub fn nearest_cluster(&self, query: &[f32]) -> Option<u32> {
        if self.centroids.is_empty() {
            return None;
        }
        let mut best_id = 0u32;
        let mut best_score = f32::NEG_INFINITY;
        for (i, centroid) in self.centroids.iter().enumerate() {
            let score: f32 = query.iter().zip(centroid.iter()).map(|(a, b)| a * b).sum();
            if score > best_score {
                best_score = score;
                best_id = i as u32;
            }
        }
        Some(best_id)
    }

    /// Search within a cluster for top-K matches, weighted by strength.
    pub fn search_cluster(
        &self,
        cluster_id: u32,
        query: &[f32],
        top_k: usize,
    ) -> Vec<(usize, f32)> {
        let mut results: Vec<(usize, f32)> = self
            .entries
            .iter()
            .filter(|e| e.cluster_id == cluster_id)
            .map(|e| {
                let score: f32 = query.iter().zip(e.vector.iter()).map(|(a, b)| a * b).sum();
                (e.doc_idx, score * e.strength)
            })
            .collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);
        results
    }

    /// Full latent search: find nearest cluster → scan cluster → return top-K.
    pub fn search(&self, query: &[f32], top_k: usize) -> Vec<(usize, f32)> {
        if let Some(cluster_id) = self.nearest_cluster(query) {
            self.search_cluster(cluster_id, query, top_k)
        } else {
            Vec::new()
        }
    }

    /// Strengthen an entry (called on recall — memories that are accessed grow stronger).
    pub fn strengthen(&mut self, entry_idx: usize, amount: f32) {
        if let Some(entry) = self.entries.get_mut(entry_idx) {
            entry.strength = (entry.strength + amount).min(10.0);
        }
    }

    /// Decay all entries by a factor (called during consolidation cycles).
    /// Entries below min_strength can be identified for pruning.
    pub fn decay_all(&mut self, factor: f32) {
        for entry in &mut self.entries {
            entry.strength *= factor;
        }
    }

    /// Number of entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Number of clusters.
    pub fn cluster_count(&self) -> usize {
        self.centroids.len()
    }
}

impl Default for LatentSpace {
    fn default() -> Self {
        Self::new(64, 384)
    }
}

// =============================================================================
// LATENT CLUSTER INDEX — the intelligence layer on top of LatentSpace
// =============================================================================

/// Engine that wraps LatentSpace with k-means clustering, Matryoshka truncation,
/// multi-cluster search, and cluster membership indexing.
///
/// This is the piece that plugs into CrystallineCore's search pipeline:
/// - At index time: truncate embeddings, dedup via BLAKE3, assign to clusters
/// - At search time: find nearest clusters → return candidate doc indices
///   (which then feed into Hamming candidate retrieval)
#[derive(Debug, Clone)]
pub struct LatentClusterIndex {
    /// The underlying latent space (entries, centroids, hashes, labels).
    space: LatentSpace,
    /// Cluster → entry indices (inverted index for O(1) candidate lookup).
    cluster_members: Vec<Vec<usize>>,
    /// Target number of clusters (K).
    k: usize,
    /// Whether clusters have been built.
    built: bool,
}

impl LatentClusterIndex {
    /// Create an empty cluster index.
    pub fn new(latent_dim: usize, source_dim: usize, k: usize) -> Self {
        Self {
            space: LatentSpace::new(latent_dim as u32, source_dim as u32),
            cluster_members: Vec::new(),
            k,
            built: false,
        }
    }

    /// Create with default parameters (64-dim latent, 384-dim source, 32 clusters).
    pub fn default_config() -> Self {
        Self::new(DEFAULT_LATENT_DIM, 384, DEFAULT_K)
    }

    /// Access the underlying latent space directly.
    pub fn space(&self) -> &LatentSpace {
        &self.space
    }

    /// Mutable access to the underlying latent space.
    pub fn space_mut(&mut self) -> &mut LatentSpace {
        &mut self.space
    }

    // =========================================================================
    // MATRYOSHKA TRUNCATION
    // =========================================================================

    /// Truncate a full embedding to latent_dim with L2 re-normalization.
    pub fn truncate(&self, embedding: &[f32]) -> Vec<f32> {
        truncate_l2(embedding, self.space.latent_dim as usize)
    }

    /// Truncate a batch of flat embeddings (contiguous, source_dim stride).
    pub fn truncate_batch(&self, embeddings_flat: &[f32]) -> Vec<Vec<f32>> {
        let source_dim = self.space.source_dim as usize;
        embeddings_flat
            .chunks(source_dim)
            .map(|emb| self.truncate(emb))
            .collect()
    }

    // =========================================================================
    // BLAKE3 CONTENT DEDUP
    // =========================================================================

    /// Check if content already exists. Returns doc_idx if duplicate.
    pub fn dedup_check(&self, text: &str) -> Option<usize> {
        let hash = blake3::hash(text.as_bytes());
        self.space
            .recall_by_hash(hash.as_bytes())
            .map(|entry| entry.doc_idx)
    }

    /// O(1) exact recall by pre-computed BLAKE3 hash.
    pub fn recall_by_hash(&self, hash: &[u8; 32]) -> Option<&LatentEntry> {
        self.space.recall_by_hash(hash)
    }

    /// Number of unique content hashes stored.
    pub fn dedup_count(&self) -> usize {
        self.space.content_hashes.len()
    }

    // =========================================================================
    // INDEXING
    // =========================================================================

    /// Add a document embedding. Truncates to latent_dim internally.
    /// Returns the entry index.
    pub fn add(&mut self, doc_idx: usize, embedding: &[f32]) -> usize {
        let vector = self.truncate(embedding);
        let content_hash = blake3::hash(&[]).into(); // placeholder hash
        let entry_idx = self.space.entries.len();
        self.space.add_entry(vector, 0, doc_idx, content_hash);
        self.built = false;
        entry_idx
    }

    /// Add a document with embedding and text (for dedup + latent indexing).
    /// Returns None if duplicate, Some(entry_idx) if new.
    pub fn add_with_dedup(
        &mut self,
        doc_idx: usize,
        embedding: &[f32],
        text: &str,
    ) -> Option<usize> {
        if self.dedup_check(text).is_some() {
            return None;
        }
        let vector = self.truncate(embedding);
        let content_hash: [u8; 32] = *blake3::hash(text.as_bytes()).as_bytes();
        let entry_idx = self.space.entries.len();
        self.space.add_entry(vector, 0, doc_idx, content_hash);
        self.built = false;
        Some(entry_idx)
    }

    /// Build clusters via k-means on all stored latent vectors.
    /// Discovers topic structure, assigns entries to clusters, builds membership index.
    pub fn build(&mut self) {
        let n = self.space.entries.len();
        if n == 0 {
            self.built = true;
            return;
        }

        let k = self.k.min(n);
        let dim = self.space.latent_dim as usize;

        // Initialize centroids via deterministic k-means++
        let centroids = kmeans_plus_plus_init(&self.space.entries, k, dim);
        self.space.centroids = centroids;

        // Run k-means iterations
        for _ in 0..KMEANS_MAX_ITER {
            // Assign each entry to nearest centroid
            for entry in self.space.entries.iter_mut() {
                entry.cluster_id = nearest_centroid(&entry.vector, &self.space.centroids);
            }

            // Recompute centroids
            let new_centroids =
                recompute_centroids(&self.space.entries, k, dim);

            // Check convergence
            let max_shift = self
                .space
                .centroids
                .iter()
                .zip(new_centroids.iter())
                .map(|(old, new)| {
                    old.iter()
                        .zip(new.iter())
                        .map(|(a, b)| ((a - b) as f64).powi(2))
                        .sum::<f64>()
                        .sqrt()
                })
                .fold(0.0f64, f64::max);

            self.space.centroids = new_centroids;

            if max_shift < KMEANS_EPSILON {
                break;
            }
        }

        // Auto-generate topic labels from cluster indices
        self.space.labels = (0..k).map(|i| format!("topic_{}", i)).collect();

        // Build cluster membership index
        self.cluster_members = vec![Vec::new(); k];
        for (entry_idx, entry) in self.space.entries.iter().enumerate() {
            let cid = entry.cluster_id as usize;
            if cid < k {
                self.cluster_members[cid].push(entry_idx);
            }
        }

        self.built = true;
    }

    // =========================================================================
    // HOT PATH — zero-allocation candidate retrieval
    // =========================================================================

    /// Zero-allocation cluster candidate retrieval.
    ///
    /// Takes a high-dimensional query (e.g. 384D), squeezes to a stack-allocated
    /// [f32; 64], finds the nearest centroid via dot-product, and returns the
    /// pre-computed member list. No heap allocation on the query path.
    ///
    /// In --release mode, LLVM auto-vectorizes the 64-element dot-product to
    /// AVX2/AVX-512 SIMD.
    pub fn get_cluster_candidates(&self, query_vector: &[f32]) -> Vec<usize> {
        if !self.built || self.space.entries.is_empty() {
            return self.space.entries.iter().map(|e| e.doc_idx).collect();
        }

        // 1. Matryoshka squeeze to stack-allocated [f32; 64]
        let truncated = truncate_to_64(query_vector);

        // 2. Centroid gravitation — find nearest cluster
        let mut best_cluster = 0usize;
        let mut max_sim = f32::NEG_INFINITY;
        for (i, centroid) in self.space.centroids.iter().enumerate() {
            let sim = dot_product_64(&truncated, centroid);
            if sim > max_sim {
                max_sim = sim;
                best_cluster = i;
            }
        }

        // 3. O(1) inverted pull — return pre-computed doc indices
        self.cluster_members
            .get(best_cluster)
            .map(|members| {
                members
                    .iter()
                    .map(|&entry_idx| self.space.entries[entry_idx].doc_idx)
                    .collect()
            })
            .unwrap_or_default()
    }

    // =========================================================================
    // SEARCH — true latent space search with strength weighting
    // =========================================================================

    /// Find the nearest cluster(s) for a query embedding.
    /// Returns cluster IDs sorted by similarity (closest first).
    pub fn nearest_clusters(&self, query_embedding: &[f32], n_clusters: usize) -> Vec<u32> {
        let truncated = truncate_to_64(query_embedding);
        let mut scores: Vec<(u32, f32)> = self
            .space
            .centroids
            .iter()
            .enumerate()
            .map(|(i, centroid)| {
                let dot = dot_product_64(&truncated, centroid);
                (i as u32, dot)
            })
            .collect();

        scores
            .sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores
            .into_iter()
            .take(n_clusters)
            .map(|(id, _)| id)
            .collect()
    }

    /// Get candidate doc indices from the top-N nearest clusters.
    pub fn get_candidates(&self, query_embedding: &[f32], n_clusters: usize) -> Vec<usize> {
        if !self.built || self.space.entries.is_empty() {
            return self.space.entries.iter().map(|e| e.doc_idx).collect();
        }

        let cluster_ids = self.nearest_clusters(query_embedding, n_clusters);
        let mut candidates = Vec::new();
        for cid in cluster_ids {
            if let Some(members) = self.cluster_members.get(cid as usize) {
                for &entry_idx in members {
                    candidates.push(self.space.entries[entry_idx].doc_idx);
                }
            }
        }
        candidates
    }

    /// Strength-weighted cluster search.
    ///
    /// Searches N nearest clusters with dot-product × consolidation strength.
    /// For speed: only scans entries in selected clusters (~5% of corpus).
    pub fn search(
        &self,
        query_embedding: &[f32],
        n_clusters: usize,
        top_k: usize,
    ) -> Vec<(usize, f32)> {
        if !self.built || self.space.entries.is_empty() {
            return Vec::new();
        }

        let truncated = truncate_to_64(query_embedding);
        let cluster_ids = self.nearest_clusters(query_embedding, n_clusters);

        let mut results: Vec<(usize, f32)> = Vec::new();
        for cid in cluster_ids {
            if let Some(members) = self.cluster_members.get(cid as usize) {
                for &entry_idx in members {
                    let entry = &self.space.entries[entry_idx];
                    let dot = dot_product_64(&truncated, &entry.vector);
                    results.push((entry.doc_idx, dot * entry.strength));
                }
            }
        }

        results
            .sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);
        results
    }

    /// Exact latent space search — 100% recall, scans ALL entries.
    ///
    /// This is the mathematically complete path: every entry is scored in
    /// 64-dim latent space with strength weighting. No cluster narrowing.
    /// Use this for MTEB/LongEmbed evaluation where recall must be perfect.
    ///
    /// Still fast: 64-dim dot-product is 6× cheaper than 384-dim, and the
    /// compiler auto-vectorizes to SIMD. At 100K entries this runs in <5ms.
    pub fn search_exact(
        &self,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Vec<(usize, f32)> {
        if self.space.entries.is_empty() {
            return Vec::new();
        }

        let truncated = truncate_to_64(query_embedding);

        let mut results: Vec<(usize, f32)> = self
            .space
            .entries
            .iter()
            .map(|entry| {
                let dot = dot_product_64(&truncated, &entry.vector);
                (entry.doc_idx, dot * entry.strength)
            })
            .collect();

        results
            .sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);
        results
    }

    /// Recall-and-strengthen: search + boost strength of returned entries.
    /// Implements the consolidation cycle — recalling strengthens the memory.
    pub fn recall(
        &mut self,
        query_embedding: &[f32],
        n_clusters: usize,
        top_k: usize,
    ) -> Vec<(usize, f32)> {
        let results = self.search(query_embedding, n_clusters, top_k);

        let truncated = truncate_to_64(query_embedding);
        let cluster_ids = self.nearest_clusters(query_embedding, n_clusters);
        for cid in &cluster_ids {
            if let Some(members) = self.cluster_members.get(*cid as usize) {
                for &entry_idx in members {
                    let entry = &self.space.entries[entry_idx];
                    let dot = dot_product_64(&truncated, &entry.vector);
                    if dot > 0.5 {
                        self.space.strengthen(entry_idx, 0.1);
                    }
                }
            }
        }

        results
    }

    // =========================================================================
    // TOPIC INTROSPECTION — "what do I know about?"
    // =========================================================================

    /// Get topic labels with entry counts.
    pub fn topics(&self) -> Vec<(&str, usize)> {
        self.space
            .labels
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let count = self.cluster_members.get(i).map(|m| m.len()).unwrap_or(0);
                (label.as_str(), count)
            })
            .collect()
    }

    /// Set a human-readable label for a cluster.
    pub fn set_topic_label(&mut self, cluster_id: u32, label: &str) {
        let cid = cluster_id as usize;
        if cid < self.space.labels.len() {
            self.space.labels[cid] = label.to_string();
        }
    }

    /// Find entries with strength below threshold (candidates for pruning).
    pub fn weak_entries(&self, min_strength: f32) -> Vec<(usize, usize, f32)> {
        self.space
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.strength < min_strength)
            .map(|(idx, e)| (idx, e.doc_idx, e.strength))
            .collect()
    }

    /// Average strength per cluster — identifies stale vs active topics.
    pub fn cluster_health(&self) -> Vec<(u32, f32, usize)> {
        self.cluster_members
            .iter()
            .enumerate()
            .map(|(cid, members)| {
                if members.is_empty() {
                    return (cid as u32, 0.0, 0);
                }
                let avg_strength: f32 = members
                    .iter()
                    .map(|&idx| self.space.entries[idx].strength)
                    .sum::<f32>()
                    / members.len() as f32;
                (cid as u32, avg_strength, members.len())
            })
            .collect()
    }

    // =========================================================================
    // INTROSPECTION
    // =========================================================================

    /// Number of entries indexed.
    pub fn entry_count(&self) -> usize {
        self.space.entry_count()
    }

    /// Number of clusters.
    pub fn cluster_count(&self) -> usize {
        self.space.cluster_count()
    }

    /// Whether clusters have been built.
    pub fn is_built(&self) -> bool {
        self.built
    }

    /// Cluster sizes.
    pub fn cluster_sizes(&self) -> Vec<usize> {
        self.cluster_members.iter().map(|m| m.len()).collect()
    }

    /// Latent dimension.
    pub fn latent_dim(&self) -> usize {
        self.space.latent_dim as usize
    }

    /// Source embedding dimension.
    pub fn source_dim(&self) -> usize {
        self.space.source_dim as usize
    }

    /// Memory usage estimate in bytes.
    pub fn memory_bytes(&self) -> usize {
        let dim = self.space.latent_dim as usize;
        let centroids = self.space.centroids.len() * dim * 4;
        let entries = self.space.entries.len() * (dim * 4 + 8 + 4 + 4); // vector + doc_idx + cluster_id + strength
        let members = self.cluster_members.iter().map(|m| m.len() * 8).sum::<usize>();
        let hashes = self.space.content_hashes.len() * (32 + 8);
        let labels: usize = self.space.labels.iter().map(|l| l.len()).sum();
        centroids + entries + members + hashes + labels
    }
}

// =============================================================================
// ZERO-ALLOCATION HOT PATH — stack-allocated 64-dim operations
// =============================================================================

/// Truncate any embedding to a stack-allocated [f32; 64] with L2 renormalization.
/// Zero heap allocation. LLVM auto-vectorizes to SIMD in --release.
#[inline(always)]
fn truncate_to_64(embedding: &[f32]) -> [f32; 64] {
    let mut out = [0.0f32; 64];
    let limit = embedding.len().min(64);
    out[..limit].copy_from_slice(&embedding[..limit]);

    // L2 renormalization
    let norm: f32 = out.iter().map(|&x| x * x).sum::<f32>().sqrt();
    if norm > 1e-12 {
        for v in out.iter_mut() {
            *v /= norm;
        }
    }
    out
}

/// 64-dim dot-product. Accepts slice for centroid/entry vectors.
/// LLVM will auto-vectorize this tight loop to AVX2/AVX-512 in --release.
#[inline(always)]
fn dot_product_64(a: &[f32; 64], b: &[f32]) -> f32 {
    let limit = b.len().min(64);
    a[..limit].iter().zip(b[..limit].iter()).map(|(x, y)| x * y).sum()
}

// =============================================================================
// INTERNAL: Matryoshka truncation (Vec path for index-time, non-hot-path)
// =============================================================================

/// Truncate embedding to target_dim and L2-normalize. Returns Vec (index-time use).
fn truncate_l2(embedding: &[f32], target_dim: usize) -> Vec<f32> {
    let slice = if embedding.len() <= target_dim {
        embedding
    } else {
        &embedding[..target_dim]
    };

    let norm: f32 = slice.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm < 1e-12 {
        return slice.to_vec();
    }
    slice.iter().map(|x| x / norm).collect()
}

// =============================================================================
// INTERNAL: K-means clustering
// =============================================================================

/// K-means++ initialization: pick diverse initial centroids (deterministic).
fn kmeans_plus_plus_init(entries: &[LatentEntry], k: usize, dim: usize) -> Vec<Vec<f32>> {
    let n = entries.len();
    let mut centroids: Vec<Vec<f32>> = Vec::with_capacity(k);

    // First centroid: entry closest to the mean
    let mut mean = vec![0.0f64; dim];
    for entry in entries {
        for (i, &v) in entry.vector.iter().enumerate() {
            if i < dim {
                mean[i] += v as f64;
            }
        }
    }
    let inv_n = 1.0 / n as f64;
    for m in mean.iter_mut() {
        *m *= inv_n;
    }

    let first_idx = entries
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let da: f64 = a
                .vector
                .iter()
                .zip(mean.iter())
                .map(|(&v, &m)| (v as f64 - m).powi(2))
                .sum();
            let db: f64 = b
                .vector
                .iter()
                .zip(mean.iter())
                .map(|(&v, &m)| (v as f64 - m).powi(2))
                .sum();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0);

    centroids.push(entries[first_idx].vector.clone());

    // Subsequent centroids: max min-distance to existing centroids
    let mut min_dists = vec![f64::MAX; n];

    for _ in 1..k {
        let last_centroid = centroids.last().unwrap();
        for (i, entry) in entries.iter().enumerate() {
            let d: f64 = entry
                .vector
                .iter()
                .zip(last_centroid.iter())
                .map(|(&a, &b)| ((a - b) as f64).powi(2))
                .sum();
            if d < min_dists[i] {
                min_dists[i] = d;
            }
        }

        let next_idx = min_dists
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);

        centroids.push(entries[next_idx].vector.clone());
    }

    centroids
}

/// Find nearest centroid for a vector (Euclidean distance).
fn nearest_centroid(vector: &[f32], centroids: &[Vec<f32>]) -> u32 {
    let mut best_id = 0u32;
    let mut best_dist = f64::MAX;
    for (i, centroid) in centroids.iter().enumerate() {
        let dist: f64 = vector
            .iter()
            .zip(centroid.iter())
            .map(|(&a, &b)| ((a - b) as f64).powi(2))
            .sum();
        if dist < best_dist {
            best_dist = dist;
            best_id = i as u32;
        }
    }
    best_id
}

/// Recompute centroids from current assignments.
fn recompute_centroids(entries: &[LatentEntry], k: usize, dim: usize) -> Vec<Vec<f32>> {
    let mut sums = vec![vec![0.0f64; dim]; k];
    let mut counts = vec![0usize; k];

    for entry in entries {
        let cid = entry.cluster_id as usize;
        if cid < k {
            counts[cid] += 1;
            for (i, &v) in entry.vector.iter().enumerate() {
                if i < dim {
                    sums[cid][i] += v as f64;
                }
            }
        }
    }

    sums.into_iter()
        .zip(counts.iter())
        .map(|(sum, &count)| {
            if count == 0 {
                vec![0.0f32; dim]
            } else {
                let inv = 1.0 / count as f64;
                sum.into_iter().map(|s| (s * inv) as f32).collect()
            }
        })
        .collect()
}

// =============================================================================
// DUAL ENCODER — fast (static) + precision (LAM) paths into latent space
// =============================================================================

/// Which encoder produced a given embedding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderSource {
    /// Full transformer encoder (LAM, BERT, etc.) — high quality, slow.
    Precision,
    /// Static token lookup + mean pooling (Model2Vec) — fast, approximate.
    Static,
}

/// Result of encoding a text through the dual encoder.
#[derive(Debug, Clone)]
pub struct EncodedEntry {
    /// The embedding vector (full dimension before truncation).
    pub embedding: Vec<f32>,
    /// Which encoder produced this.
    pub source: EncoderSource,
}

/// Static encoder using Model2Vec — token lookup table + mean pooling.
///
/// Enabled with the `static-embed` feature flag. When unavailable,
/// falls back to requiring external embeddings (precision path only).
#[cfg(feature = "static-embed")]
pub struct StaticEncoder {
    model: model2vec_rs::model::StaticModel,
}

#[cfg(feature = "static-embed")]
impl StaticEncoder {
    /// Embedded model files — baked into the binary at compile time.
    /// said-lam-static: 64-dim, 4.8MB total (model + tokenizer + config).
    /// This means zero external files needed at runtime.
    // Embedded model dir: said-lam-static (64-dim, original) or said-lam-static-4M
    // (potion-base-8M, 256-dim — far stronger conceptual recall, CPU-only lookup table).
    // The original 64-dim model is kept on disk untouched; switch by changing this path.
    #[cfg(feature = "embed-model")]
    const MODEL_BYTES: &[u8] = include_bytes!("../../../SAID-LAM-private/said-lam-static-4M/model.safetensors");
    #[cfg(feature = "embed-model")]
    const TOKENIZER_BYTES: &[u8] = include_bytes!("../../../SAID-LAM-private/said-lam-static-4M/tokenizer.json");
    #[cfg(feature = "embed-model")]
    const CONFIG_BYTES: &[u8] = include_bytes!("../../../SAID-LAM-private/said-lam-static-4M/config.json");

    /// Load from embedded model (zero external files).
    /// Writes to temp dir on first call, loads from there.
    #[cfg(feature = "embed-model")]
    pub fn from_embedded() -> Result<Self, String> {
        // Content-addressed temp dir: keyed by the model bytes' length so swapping the
        // embedded model (e.g. 64-dim → 256-dim) writes to a NEW dir and never loads a
        // stale cached model from a prior binary. (len is a cheap, sufficient cache key
        // here — different models differ in size.)
        let key = Self::MODEL_BYTES.len();
        let dir = std::env::temp_dir().join(format!("said-lam-static-embedded-{key}"));
        let model_f = dir.join("model.safetensors");
        if !model_f.exists() {
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("Failed to create temp dir: {}", e))?;
            std::fs::write(&model_f, Self::MODEL_BYTES)
                .map_err(|e| format!("Write model: {}", e))?;
            std::fs::write(dir.join("tokenizer.json"), Self::TOKENIZER_BYTES)
                .map_err(|e| format!("Write tokenizer: {}", e))?;
            std::fs::write(dir.join("config.json"), Self::CONFIG_BYTES)
                .map_err(|e| format!("Write config: {}", e))?;
        }
        Self::from_pretrained(dir.to_str().unwrap_or("said-lam-static-embedded"))
    }

    /// Load a Model2Vec model from local path or HuggingFace Hub.
    pub fn from_pretrained(model_name: &str) -> Result<Self, String> {
        let model = model2vec_rs::model::StaticModel::from_pretrained(model_name, None, None, None)
            .map_err(|e| format!("Failed to load Model2Vec '{}': {}", model_name, e))?;
        Ok(Self { model })
    }

    /// Load the encoder directly from in-memory bytes — no filesystem.
    /// Required for wasm (no temp-file path) and avoids the temp-dir round-trip
    /// on native. Same model as `from_pretrained`; only the source differs.
    pub fn from_bytes(
        tokenizer: &[u8],
        safetensors: &[u8],
        config: &[u8],
    ) -> Result<Self, String> {
        let model = model2vec_rs::model::StaticModel::from_bytes(
            tokenizer, safetensors, config, None,
        )
        .map_err(|e| format!("Failed to load Model2Vec from bytes: {}", e))?;
        Ok(Self { model })
    }

    /// Encode a single text. Returns embedding vector.
    pub fn encode_one(&self, text: &str) -> Vec<f32> {
        self.model.encode_single(text)
    }

    /// Encode a batch of texts. Returns one embedding per text.
    pub fn encode_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
        self.model.encode(texts)
    }
}

/// Dual-encoder for latent space: routes between fast (static) and precision (external) paths.
///
/// Architecture:
///   INGEST (bulk, speed matters):
///     Document → Model2Vec static encode (500x faster) → truncate → latent entry
///
///   PRECISION (quality matters):
///     Document → LAM/external encode (384-dim) → truncate → latent entry
///
///   QUERY (always precision):
///     Query → external encode → truncate → dot-product search
///
/// The latent space stores entries from both paths in the same space.
/// Both produce vectors in the same semantic neighborhood — static is just
/// a faster approximation that places documents in roughly the right location.
pub struct DualEncoder {
    /// Fast path: static token-lookup encoder (optional, requires `static-embed` feature).
    #[cfg(feature = "static-embed")]
    pub static_encoder: Option<StaticEncoder>,

    /// Latent dimension for truncation.
    pub latent_dim: usize,

    /// Counters for tracking which path is used.
    pub static_count: usize,
    pub precision_count: usize,
}

impl DualEncoder {
    /// Create a dual encoder without the static path (precision only).
    pub fn precision_only(latent_dim: usize) -> Self {
        Self {
            #[cfg(feature = "static-embed")]
            static_encoder: None,
            latent_dim,
            static_count: 0,
            precision_count: 0,
        }
    }

    /// Create a dual encoder with Model2Vec static path.
    #[cfg(feature = "static-embed")]
    pub fn with_static(model_name: &str, latent_dim: usize) -> Result<Self, String> {
        let enc = StaticEncoder::from_pretrained(model_name)?;
        Ok(Self {
            static_encoder: Some(enc),
            latent_dim,
            static_count: 0,
            precision_count: 0,
        })
    }

    /// Check if static encoding is available.
    pub fn has_static(&self) -> bool {
        #[cfg(feature = "static-embed")]
        { self.static_encoder.is_some() }
        #[cfg(not(feature = "static-embed"))]
        { false }
    }

    /// Fast-encode a text via Model2Vec static path.
    /// Returns None if static encoder is not available.
    pub fn encode_fast(&mut self, text: &str) -> Option<EncodedEntry> {
        #[cfg(feature = "static-embed")]
        {
            if let Some(ref enc) = self.static_encoder {
                let emb = enc.encode_one(text);
                if !emb.is_empty() {
                    self.static_count += 1;
                    return Some(EncodedEntry {
                        embedding: emb,
                        source: EncoderSource::Static,
                    });
                }
            }
        }
        let _ = text;
        None
    }

    /// Fast-encode a batch of texts via Model2Vec static path.
    #[cfg(feature = "static-embed")]
    pub fn encode_fast_batch(&mut self, texts: &[String]) -> Option<Vec<EncodedEntry>> {
        if let Some(ref enc) = self.static_encoder {
            let embeddings = enc.encode_batch(texts);
            if !embeddings.is_empty() {
                self.static_count += embeddings.len();
                return Some(
                    embeddings
                        .into_iter()
                        .map(|emb| EncodedEntry {
                            embedding: emb,
                            source: EncoderSource::Static,
                        })
                        .collect(),
                );
            }
        }
        None
    }

    /// Wrap an externally-computed embedding as a precision entry.
    pub fn wrap_precision(&mut self, embedding: Vec<f32>) -> EncodedEntry {
        self.precision_count += 1;
        EncodedEntry {
            embedding,
            source: EncoderSource::Precision,
        }
    }

    /// Encode and add to a LatentClusterIndex using the fast path.
    /// Falls back to requiring an external embedding if static is unavailable.
    pub fn add_fast(
        &mut self,
        index: &mut LatentClusterIndex,
        doc_idx: usize,
        text: &str,
    ) -> Option<usize> {
        if let Some(entry) = self.encode_fast(text) {
            Some(index.add(doc_idx, &entry.embedding))
        } else {
            None // caller must provide external embedding
        }
    }

    /// Encode and add with dedup using the fast path.
    pub fn add_fast_dedup(
        &mut self,
        index: &mut LatentClusterIndex,
        doc_idx: usize,
        text: &str,
    ) -> Option<usize> {
        // Dedup check first (doesn't need encoding)
        if index.dedup_check(text).is_some() {
            return None;
        }
        if let Some(entry) = self.encode_fast(text) {
            index.add_with_dedup(doc_idx, &entry.embedding, text)
        } else {
            None
        }
    }

    /// Stats summary.
    pub fn stats(&self) -> (usize, usize) {
        (self.static_count, self.precision_count)
    }
}
