//! ═══════════════════════════════════════════════════════════════════════════════
//!                     SAID Crystalline Attention (SCA)
//!                     Pluggable Search Engine — Model-Agnostic
//! ═══════════════════════════════════════════════════════════════════════════════
//!
//! CrystallineCore is the production search engine extracted from SAID-LAM.
//! It takes pre-computed embeddings (from ANY model) and provides:
//!
//! - ART (Adaptive Radix Tree) inverted index for O(1) token lookup
//! - 1-bit holographic quantization (16-view) for Hamming search
//! - IDF-weighted hybrid scoring with phrase matching
//! - 3-path auto-routing: PureLexical / PureSemantic / FullHybrid
//! - BERT tokenizer integration (optional) for fine-grained token indexing
//!
//! ## Usage (pluggable — no model dependency):
//! ```ignore
//! let mut engine = CrystallineCore::new();
//! engine.set_corpus_mean(mean_vec);
//! engine.load_idf_fast(words, scores);
//! engine.add_docs_quantized(ids, embeddings_flat, passage_counts, gammas, doc_words);
//! let results = engine.search_unified_quantized(&query_emb, "query text", 10);
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use regex::Regex;
#[cfg(feature = "bert")]
use tokenizers::Tokenizer;
use roaring::RoaringBitmap;
// SCA drop-in alignment: quantized Hamming, parallel processing
use rayon::prelude::*;
use ahash::{AHashMap, AHashSet};
#[cfg(feature = "simd")]
use simsimd::BinarySimilarity;
use rust_stemmers::{Algorithm, Stemmer};

// Text storage (inlined in sca-core, no external dependency)
use crate::storage::TextStorage;

// ═══════════════════════════════════════════════════════════════════════════════
// Hamming distance fallback (when simsimd is not available)
// ═══════════════════════════════════════════════════════════════════════════════

/// Trait providing Hamming distance on byte slices.
/// When `simd` feature is enabled, simsimd's BinarySimilarity is used instead.
#[cfg(not(feature = "simd"))]
trait HammingDistance {
    fn hamming(a: &[u8], b: &[u8]) -> Option<f64>;
}

#[cfg(not(feature = "simd"))]
impl HammingDistance for u8 {
    fn hamming(a: &[u8], b: &[u8]) -> Option<f64> {
        if a.len() != b.len() { return None; }
        let dist: u32 = a.iter().zip(b.iter())
            .map(|(x, y)| (x ^ y).count_ones())
            .sum();
        Some(dist as f64)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tokenizer stub (when bert feature is not available)
// ═══════════════════════════════════════════════════════════════════════════════

/// When the `bert` feature is disabled, the tokenizer field is always None.
/// This type alias allows the struct to compile without the tokenizers crate.
#[cfg(not(feature = "bert"))]
type Tokenizer = ();

// ═══════════════════════════════════════════════════════════════════════════════
// ADAPTIVE RADIX TREE (ART) - Integrated Inverted Index
// ═══════════════════════════════════════════════════════════════════════════════
//
// O(4) constant-time lookup (token_id = 4 bytes)
// Pattern matching support: prefix_search(), find_all_patterns()
// Cache-friendly traversal for large document collections
//
// ═══════════════════════════════════════════════════════════════════════════════

/// Simple radix tree node for inverted index
#[derive(Clone, Default)]
struct RadixNode {
    children: HashMap<u8, Box<RadixNode>>,  // byte → child
    doc_ids: Option<HashSet<String>>,       // Leaf: doc_ids
    positions: Option<HashMap<String, Vec<usize>>>, // Leaf: {doc_id: [pos1, pos2, ...]}
}

impl RadixNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            doc_ids: None,
            positions: None,
        }
    }
}

/// Adaptive Radix Tree for inverted index WITH POSITIONS.
/// 
/// Maps: token_id (u32) → {doc_id: [positions]}
/// 
/// O(4) = O(1) lookup, insert, delete (4 bytes = constant)
#[derive(Clone)]
#[allow(dead_code)]
pub struct ART {
    root: Box<RadixNode>,
    size: usize,
    postings: usize,
}

#[allow(dead_code)]
impl ART {
    pub fn new() -> Self {
        Self {
            root: Box::new(RadixNode::new()),
            size: 0,
            postings: 0,
        }
    }
    
    /// Insert doc_id and position for token_id
    pub fn insert(&mut self, token_id: u32, doc_id: &str, position: Option<usize>) {
        let key = token_id.to_be_bytes();
        let mut node = &mut *self.root;
        
        for &byte in &key {
            node = node.children
                .entry(byte)
                .or_insert_with(|| Box::new(RadixNode::new()));
        }
        
        if node.doc_ids.is_none() {
            node.doc_ids = Some(HashSet::new());
            node.positions = Some(HashMap::new());
            self.size += 1;
        }
        
        let doc_ids = node.doc_ids.as_mut().unwrap();
        let positions = node.positions.as_mut().unwrap();
        
        if !doc_ids.contains(doc_id) {
            doc_ids.insert(doc_id.to_string());
            positions.insert(doc_id.to_string(), Vec::new());
            self.postings += 1;
        }
        
        if let Some(pos) = position {
            if let Some(pos_list) = positions.get_mut(doc_id) {
                pos_list.push(pos);
            }
        }
    }
    
    /// Get doc_ids for token_id
    pub fn get(&self, token_id: u32) -> HashSet<String> {
        let key = token_id.to_be_bytes();
        let mut node = &*self.root;
        
        for &byte in &key {
            match node.children.get(&byte) {
                Some(child) => node = child,
                None => return HashSet::new(),
            }
        }
        
        node.doc_ids.clone().unwrap_or_default()
    }
    
    /// Get positions for token_id: {doc_id: [pos1, pos2, ...]}
    pub fn get_positions(&self, token_id: u32) -> HashMap<String, Vec<usize>> {
        let key = token_id.to_be_bytes();
        let mut node = &*self.root;
        
        for &byte in &key {
            match node.children.get(&byte) {
                Some(child) => node = child,
                None => return HashMap::new(),
            }
        }
        
        node.positions.clone().unwrap_or_default()
    }
    
    /// Get document frequency for token_id (how many docs contain it)
    pub fn get_doc_freq(&self, token_id: u32) -> usize {
        self.get(token_id).len()
    }
    
    /// Remove doc_id from token_id's posting list
    pub fn remove(&mut self, token_id: u32, doc_id: &str) -> bool {
        let key = token_id.to_be_bytes();
        let mut node = &mut *self.root;
        let mut path: Vec<(*mut RadixNode, u8)> = Vec::new();
        
        for &byte in &key {
            let node_ptr = node as *mut RadixNode;
            match node.children.get_mut(&byte) {
                Some(child) => {
                    path.push((node_ptr, byte));
                    node = child;
                }
                None => return false,
            }
        }
        
        if let Some(ref mut doc_ids) = node.doc_ids {
            if !doc_ids.contains(doc_id) {
                return false;
            }
            
            doc_ids.remove(doc_id);
            self.postings -= 1;
            
            if let Some(ref mut positions) = node.positions {
                positions.remove(doc_id);
            }
            
            // Cleanup empty nodes
            if doc_ids.is_empty() && node.children.is_empty() {
                node.doc_ids = None;
                node.positions = None;
                self.size -= 1;
                
                // Remove empty parent nodes
                for (parent_ptr, byte) in path.into_iter().rev() {
                    unsafe {
                        let parent = &mut *parent_ptr;
                        if let Some(child) = parent.children.get(&byte) {
                            if child.doc_ids.is_none() && child.children.is_empty() {
                                parent.children.remove(&byte);
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
            
            true
        } else {
            false
        }
    }
    
    /// Iterate over all (token_id, doc_ids) pairs
    pub fn iter_all(&self) -> Vec<(u32, HashSet<String>)> {
        let mut results = Vec::new();
        self.iter_helper(&self.root, &mut [0u8; 4], 0, &mut results);
        results
    }
    
    fn iter_helper(
        &self,
        node: &RadixNode,
        prefix: &mut [u8; 4],
        depth: usize,
        results: &mut Vec<(u32, HashSet<String>)>,
    ) {
        if depth == 4 {
            if let Some(ref doc_ids) = node.doc_ids {
                let token_id = u32::from_be_bytes(*prefix);
                results.push((token_id, doc_ids.clone()));
            }
            return;
        }
        
        for (&byte, child) in &node.children {
            prefix[depth] = byte;
            self.iter_helper(child, prefix, depth + 1, results);
        }
    }
    
    /// O(k) Prefix Search - Tree traversal, no regex.
    /// Find all token_ids that share the same prefix bytes.
    pub fn search_prefix(&self, prefix_token_id: u32, prefix_bytes_len: usize) -> Vec<(u32, HashSet<String>)> {
        let full_key = prefix_token_id.to_be_bytes();
        let prefix = &full_key[..prefix_bytes_len.min(4)];
        
        let mut node = &*self.root;
        
        // Navigate to prefix node
        for &byte in prefix {
            match node.children.get(&byte) {
                Some(child) => node = child,
                None => return Vec::new(),
            }
        }
        
        // Yield everything below this node
        let mut results = Vec::new();
        let mut current_prefix = [0u8; 4];
        current_prefix[..prefix.len()].copy_from_slice(prefix);
        self.prefix_recurse(node, &mut current_prefix, prefix.len(), &mut results);
        results
    }
    
    fn prefix_recurse(
        &self,
        node: &RadixNode,
        path: &mut [u8; 4],
        depth: usize,
        results: &mut Vec<(u32, HashSet<String>)>,
    ) {
        if depth == 4 {
            if let Some(ref doc_ids) = node.doc_ids {
                let token_id = u32::from_be_bytes(*path);
                results.push((token_id, doc_ids.clone()));
            }
            return;
        }
        
        for (&byte, child) in &node.children {
            path[depth] = byte;
            self.prefix_recurse(child, path, depth + 1, results);
        }
    }
    
    /// Clear all data
    pub fn clear(&mut self) {
        self.root = Box::new(RadixNode::new());
        self.size = 0;
        self.postings = 0;
    }
    
    /// Get number of unique tokens
    pub fn len(&self) -> usize {
        self.size
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
}

/// Dict-like wrapper around ART for drop-in replacement of HashMap<u32, HashSet<String>>
#[derive(Clone)]
#[allow(dead_code)]
pub struct ARTDict {
    art: ART,
}

#[allow(dead_code)]
impl ARTDict {
    pub fn new() -> Self {
        Self { art: ART::new() }
    }
    
    pub fn contains(&self, token_id: u32) -> bool {
        !self.art.get(token_id).is_empty()
    }
    
    pub fn len(&self) -> usize {
        self.art.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.art.is_empty()
    }
    
    pub fn get(&self, token_id: u32) -> HashSet<String> {
        self.art.get(token_id)
    }
    
    pub fn clear(&mut self) {
        self.art.clear();
    }
    
    pub fn values(&self) -> Vec<HashSet<String>> {
        self.art.iter_all().into_iter().map(|(_, docs)| docs).collect()
    }
    
    pub fn items(&self) -> Vec<(u32, HashSet<String>)> {
        self.art.iter_all()
    }
    
    pub fn add(&mut self, token_id: u32, doc_id: &str, position: Option<usize>) {
        self.art.insert(token_id, doc_id, position);
    }
    
    pub fn get_positions(&self, token_id: u32) -> HashMap<String, Vec<usize>> {
        self.art.get_positions(token_id)
    }
    
    pub fn get_doc_freq(&self, token_id: u32) -> usize {
        self.art.get_doc_freq(token_id)
    }
    
    pub fn discard(&mut self, token_id: u32, doc_id: &str) {
        self.art.remove(token_id, doc_id);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// STOPWORDS & CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Stopwords for query filtering (matches Python _STOPWORDS exactly)
const STOPWORDS: &[&str] = &[
    "what", "when", "where", "which", "who", "why", "how", "the", "they",
    "them", "their", "known", "that", "this", "with", "from", "have", "been",
    "were", "being", "for", "was", "and", "are", "is", "his", "her", "she", "he",
    "a", "an", "of", "to", "in", "it", "on", "at", "by", "as", "or", "be",
    "do", "did", "does", "has", "had", "its",
];

/// Connector tokens (always skip in search_kv)
const CONNECTOR_TOKENS: &[&str] = &[
    "is", "are", "was", "were", "be", "been", "being",
    "the", "a", "an", "of", "to", "in", "on", "at", "by", "for", "with", "from",
    "and", "or", "as", "that", "which", "equals",
];

/// Protected values (never skip in search_kv)
#[allow(dead_code)]
const PROTECTED_VALUES: &[&str] = &[
    "true", "false", "yes", "no", "on", "off", "0", "1",
    "active", "inactive", "enabled", "disabled", "success", "failed",
    "error", "warning", "info", "critical", "high", "medium", "low",
    "null", "none", "nil", "undefined",
];

/// Code-intent words: query contains one of these (whole-word) → route to high_lexical (port from rust_test lib.rs)
const CODE_INTENT_WORDS: &[&str] = &["passkey", "password", "passcode", "serial", "needle"];

/// Known compound→split mappings for CODE_INTENT_WORDS that may appear as two
/// words in documents (e.g. "pass key") but one word in queries ("passkey").
const COMPOUND_SPLITS: &[(&str, &[&str])] = &[
    ("passkey", &["pass", "key"]),
    ("passcode", &["pass", "code"]),
    ("password", &["pass", "word"]),
];

/// Route from query analysis — determines which search path to use.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum QueryRoute {
    PureLexical,
    PureSemantic,
    FullHybrid,
}

// ═══════════════════════════════════════════════════════════════════════════════
// CrystallineCore - The IDF-Surprise search engine (1:1 match with Python)
// ═══════════════════════════════════════════════════════════════════════════════

/// CrystallineCore - SCA (Said Crystalline Attention) Core Engine
/// 
/// This is the PROTECTED CORE that gets integrated into LAM.
/// Users only see: model.index(), model.search()
/// 
/// AUTO-ROUTING SEARCH:
/// - Detects query type automatically (lexical vs semantic)
/// - Uses optimal alpha for each query
/// - Quadratic boost formula for hybrid scoring
/// 
/// STREAMING:
/// - O(N) indexing, O(1) query
/// - Constant memory for 2M+ tokens
/// - Pre-indexed filler for 500x speedup
/// CrystallineCore - SCA (Said Crystalline Attention) Core Engine
/// 
/// This is a 1:1 match with Python `_crystalline.py`.
/// Uses BERT tokenization for the inverted index (ART).
/// Uses simple word tokenization ONLY for fuzzy matching (Soundex/Levenshtein).
#[allow(dead_code)]
pub struct CrystallineCore {
    // BERT Tokenizer (uses tokenizers library, NOT transformers!)
    // Matches Python: self._tokenizer
    tokenizer: Option<Arc<Tokenizer>>,
    
    // =========================================================================
    // PHASE 1: NEW TEXT STORAGE (A/B Testing)
    // =========================================================================
    // Maps String doc_id → u64 internal index
    // This enables future migration to all-integer indexing
    pub(crate) doc_id_to_idx: HashMap<String, u64>,
    
    // New text storage using InMemoryTextStore
    // Same API will work with MmapTextStore for zero-copy disk access
    text_store: TextStorage,
    
    // =========================================================================
    // CORE STORAGE (Optimized in Phase 1-2)
    // =========================================================================
    doc_ids: Vec<String>,
    // doc_texts REMOVED - text now stored in text_store (Phase 1 cleanup)
    // PHASE 2: RoaringBitmap replaces HashSet<u32> - ~100x smaller memory footprint
    doc_token_sets: HashMap<String, RoaringBitmap>,  // FORWARD INDEX: doc_id → token_set (compressed)
    doc_token_counts: HashMap<String, HashMap<u32, u32>>,  // doc_id → {token_id → count}
    doc_embeddings: HashMap<String, Vec<f32>>,
    // LEGACY LONGEMBED: passage embeddings per document for MaxSim scoring (Python _crystalline.pyx behavior)
    doc_passage_embeddings: HashMap<String, Vec<Vec<f32>>>, // doc_id -> [passage_emb, ...]
    
    // Fuzzy matching vocabulary (lazy initialization)
    token_vocab: HashMap<u32, String>,  // token_id → word
    
    // WORD-LEVEL indexes for fuzzy matching (simple tokenization, NOT BERT!)
    word_inverted_index: HashMap<String, HashSet<String>>,  // word → {doc_ids}
    word_phonetic_index: HashMap<String, HashSet<String>>, // soundex → words (Step 4: sca_dropin shape)
    word_vocabulary: HashSet<String>,                        // All unique words
    word_idf: HashMap<String, f32>,                          // word → IDF score
    word_doc_tf: HashMap<String, HashMap<String, u32>>,      // doc_id → {word → count}
    
    // INVERTED INDEX: token_id → {doc_id1, doc_id2, ...}
    // O(4) = O(1) lookup using Adaptive Radix Tree (ART)
    // Stores BERT token IDs (matches Python _inverted_index)
    inverted_index: ARTDict,
    
    // POSITION INDEX: doc_id → {position → token_id}
    // For pure ART extraction without regex
    position_tokens: HashMap<String, HashMap<usize, u32>>,
    
    // TOKEN STRINGS: token_id → string (for decoding)
    token_strings: HashMap<u32, String>,
    
    // IDF CACHE: token_id → IDF score (log(N / doc_freq))
    idf_cache: HashMap<u32, f32>,
    idf_dirty: bool,
    
    // Special tokens to skip
    special_tokens: HashSet<u32>,
    stopword_tokens: Option<HashSet<u32>>,
    
    // Config
    vocab_size: usize,
    min_token_id: u32,
    doc_count: usize,
    
    // =========================================================================
    // QUANTIZED STORAGE (sca_dropin alignment for 94.1060 parity)
    // =========================================================================
    // 1-bit quantized embeddings (all passages/docs)
    pub(crate) matrix_quantized: Vec<u8>,
    /// GPU pre-computed Hamming distances per doc (set before search, used by hybrid_candidate_retrieval)
    pub(crate) gpu_precomputed_distances: Option<Vec<u32>>,
    corpus_mean: Vec<f32>,
    /// Per-dimension std for whitened binarization: sign((emb-mean)/std)
    corpus_std: Vec<f32>,
    dim: usize,
    pub(crate) quantized_dim: usize,  // (dim + 7) / 8
    
    // Passage metadata for MaxSim
    pub(crate) passage_counts: Vec<usize>,
    pub(crate) passage_offsets: Vec<usize>,
    
    // Holographic 16-view quantization
    pub(crate) holographic_16view: bool,
    holographic_scale: f32,
    pub(crate) bytes_per_passage: usize,
    
    // Word-level lexical index (sca_dropin style: AHashMap for speed)
    // per-doc set of interned word-ids (was AHashSet<String>; interned for the #4 fix).
    doc_word_sets_fast: Vec<AHashSet<u32>>,
    // per-doc {interned word-id → term frequency} (was {String→u32}; interned for #4).
    doc_word_tf_fast: Vec<AHashMap<u32, u32>>,
    doc_texts_fast: Vec<String>,
    word_idf_fast: AHashMap<String, f32>,
    // soundex → set of interned word-ids (was AHashSet<String>; interned for the #4 fix).
    phonetic_index_fast: AHashMap<String, AHashSet<u32>>,
    // word-id → set of doc indices (was AHashMap<String,_>; interned for the #4 fix).
    word_inverted_fast: AHashMap<u32, AHashSet<usize>>,

    // Word interning (#4 OOM fix): a single canonical store of each unique word, so the
    // lexical `_fast` structures above can key on a compact `u32` id instead of duplicating
    // the same `String` ~9× across them. `word_vocab[id] == word`; `word_to_id[word] == id`.
    // Populated alongside the String structures during migration (one structure at a time),
    // so each step is independently testable against the recall + memory gates.
    word_vocab: Vec<String>,
    word_to_id: AHashMap<String, u32>,

    // Disk-backed word index (the 580MB fix): when a .said file carries a WIDX section, open()
    // stores its decompressed bytes here. Queries read postings in place via WidxReader instead of
    // the resident `_fast` HashMaps — so a large corpus never re-materializes the word index in RAM.
    // None => no WIDX (old file / fresh brain) => the resident structures / rebuild path are used.
    widx_bytes: Option<Vec<u8>>,

    // Hybrid search weights
    hybrid_alpha_semantic: f32,
    hybrid_alpha_lexical: f32,
    rerank_depth: usize,
    
    // Quantized mode flag (when true, use quantized search path)
    quantized_mode: bool,

    // Force route override (e.g. "FullHybrid" for LEMBNeedleRetrieval)
    force_route: Option<QueryRoute>,

    // QJL asymmetric search: binary docs × float queries (unbiased estimator)
    pub(crate) asymmetric_search: bool,

    // English Porter2 stemmer for morphological normalization
    // Applied at both index time and query time for consistent matching
    stemmer: Stemmer,

    // BM25 k1 parameter (term frequency saturation)
    // Standard value: 1.2 (controls how quickly TF saturates)
    // Higher = more weight on high-frequency terms
    bm25_k1: f32,
}

impl Clone for CrystallineCore {
    fn clone(&self) -> Self {
        Self {
            tokenizer: self.tokenizer.clone(),
            doc_id_to_idx: self.doc_id_to_idx.clone(),
            text_store: self.text_store.clone(),
            doc_ids: self.doc_ids.clone(),
            doc_token_sets: self.doc_token_sets.clone(),
            doc_token_counts: self.doc_token_counts.clone(),
            doc_embeddings: self.doc_embeddings.clone(),
            doc_passage_embeddings: self.doc_passage_embeddings.clone(),
            token_vocab: self.token_vocab.clone(),
            word_inverted_index: self.word_inverted_index.clone(),
            word_phonetic_index: self.word_phonetic_index.clone(),
            word_vocabulary: self.word_vocabulary.clone(),
            word_idf: self.word_idf.clone(),
            word_doc_tf: self.word_doc_tf.clone(),
            inverted_index: self.inverted_index.clone(),
            position_tokens: self.position_tokens.clone(),
            token_strings: self.token_strings.clone(),
            idf_cache: self.idf_cache.clone(),
            idf_dirty: self.idf_dirty,
            special_tokens: self.special_tokens.clone(),
            stopword_tokens: self.stopword_tokens.clone(),
            vocab_size: self.vocab_size,
            min_token_id: self.min_token_id,
            doc_count: self.doc_count,
            matrix_quantized: self.matrix_quantized.clone(),
            gpu_precomputed_distances: None,
            corpus_mean: self.corpus_mean.clone(),
            corpus_std: self.corpus_std.clone(),
            dim: self.dim,
            quantized_dim: self.quantized_dim,
            passage_counts: self.passage_counts.clone(),
            passage_offsets: self.passage_offsets.clone(),
            holographic_16view: self.holographic_16view,
            holographic_scale: self.holographic_scale,
            bytes_per_passage: self.bytes_per_passage,
            doc_word_sets_fast: self.doc_word_sets_fast.clone(),
            doc_word_tf_fast: self.doc_word_tf_fast.clone(),
            doc_texts_fast: self.doc_texts_fast.clone(),
            word_idf_fast: self.word_idf_fast.clone(),
            phonetic_index_fast: self.phonetic_index_fast.clone(),
            word_inverted_fast: self.word_inverted_fast.clone(),
            word_vocab: self.word_vocab.clone(),
            word_to_id: self.word_to_id.clone(),
            widx_bytes: self.widx_bytes.clone(),
            hybrid_alpha_semantic: self.hybrid_alpha_semantic,
            hybrid_alpha_lexical: self.hybrid_alpha_lexical,
            rerank_depth: self.rerank_depth,
            quantized_mode: self.quantized_mode,
            force_route: self.force_route,
            asymmetric_search: self.asymmetric_search,
            // Stemmer doesn't implement Clone — recreate from Algorithm
            stemmer: Stemmer::create(Algorithm::English),
            bm25_k1: self.bm25_k1,
        }
    }
}

#[allow(dead_code)]
impl CrystallineCore {
    /// Create new CrystallineCore (matches Python __init__)
    /// 
    /// Args:
    ///     tokenizer: Optional BERT tokenizer (tokenizers library)
    ///     vocab_size: Vocabulary size (default 30522 for BERT)
    ///     min_token_id: Skip special tokens below this (default 1000)
    pub fn new() -> Self {
        Self::with_tokenizer(None, 30522, 1000)
    }
    
    /// Create CrystallineCore with tokenizer (matches Python __init__)
    pub fn with_tokenizer(tokenizer: Option<Arc<Tokenizer>>, vocab_size: usize, min_token_id: u32) -> Self {
        let mut special_tokens = HashSet::new();
        special_tokens.insert(0);
        special_tokens.insert(100);
        special_tokens.insert(101);
        special_tokens.insert(102);
        special_tokens.insert(103);
        
        let default_dim = 384;  // SAID-LAM embedding dimension
        let quantized_dim = (default_dim + 7) / 8;
        
        Self {
            tokenizer,
            // Phase 1: Optimized storage
            doc_id_to_idx: HashMap::new(),
            text_store: TextStorage::new_ephemeral(),
            // Core storage
            doc_ids: Vec::new(),
            // doc_texts REMOVED - use text_store
            doc_token_sets: HashMap::new(),
            doc_token_counts: HashMap::new(),
            doc_embeddings: HashMap::new(),
            doc_passage_embeddings: HashMap::new(),
            token_vocab: HashMap::new(),
            word_inverted_index: HashMap::new(),
            word_phonetic_index: HashMap::new(),
            word_vocabulary: HashSet::new(),
            word_idf: HashMap::new(),
            word_doc_tf: HashMap::new(),
            inverted_index: ARTDict::new(),
            position_tokens: HashMap::new(),
            token_strings: HashMap::new(),
            idf_cache: HashMap::new(),
            idf_dirty: true,
            special_tokens,
            stopword_tokens: None,
            vocab_size,
            min_token_id,
            doc_count: 0,
            // Quantized storage (sca_dropin alignment)
            matrix_quantized: Vec::new(),
            gpu_precomputed_distances: None,
            corpus_mean: vec![0.0; default_dim],
            corpus_std: vec![1.0; default_dim],
            dim: default_dim,
            quantized_dim,
            passage_counts: Vec::new(),
            passage_offsets: Vec::new(),
            holographic_16view: false,
            holographic_scale: 0.2,
            bytes_per_passage: quantized_dim,
            doc_word_sets_fast: Vec::new(),
            doc_word_tf_fast: Vec::new(),
            doc_texts_fast: Vec::new(),
            word_idf_fast: AHashMap::new(),
            phonetic_index_fast: AHashMap::new(),
            word_inverted_fast: AHashMap::new(),
            word_vocab: Vec::new(),
            word_to_id: AHashMap::new(),
            widx_bytes: None,
            hybrid_alpha_semantic: 0.60,
            hybrid_alpha_lexical: 0.40,
            rerank_depth: 100,
            quantized_mode: false,
            force_route: None,
            // Asymmetric (QJL) search ON by default: rank with the FULL-PRECISION float
            // query against 1-bit doc fingerprints (14.1). Symmetric Hamming quantizes
            // the query too, flattening one entity's many memories to score ties — yet
            // float cosine on the SAME 64-dim embedding separates them 5/5
            // (test_signal_diagnostic). Asymmetric recovers that signal from the
            // existing fingerprints; it's an unbiased estimator, strictly finer than
            // symmetric. Was opt-in via a PyO3 binding only, so CLI/MCP never used it.
            asymmetric_search: true,
            stemmer: Stemmer::create(Algorithm::English),
            bm25_k1: 1.2,
        }
    }

    /// Get query IDF for adaptive alpha calculation (MTEB compatibility)
    pub fn get_query_idf(&self, query: &str) -> f32 {
        let (_, _, _, idf, _, _, _) = self.analyze_query(query);
        idf
    }

    /// Score ALL indexed docs against the query using BM25 + IDF.
    /// Uses the word-level inverted index (with Porter2 stemming applied
    /// at index time) so this is morphologically-aware.
    ///
    /// Returns a map doc_id → raw BM25 score. Docs with zero overlap are
    /// omitted from the map (not scored as zero). Caller should normalize
    /// before fusing with other score layers.
    ///
    /// Purpose: conversational and "intent" queries where the gold turn's
    /// vocabulary differs from the query's. BM25 surfaces docs that share
    /// rare-but-crucial stems (e.g. query "destress" matches doc "de-stress"
    /// after stemming + hyphen-normalization).
    pub fn bm25_score_docs(&self, query: &str) -> HashMap<String, f32> {
        // Tokenize query the same way we index (Porter2 stem, ≥2 chars).
        let q_tokens: Vec<String> = self.simple_tokenize(query)
            .into_iter()
            .filter(|t| t.len() >= 2 && !STOPWORDS.contains(&t.as_str()))
            .collect();
        if q_tokens.is_empty() || self.word_doc_tf.is_empty() {
            return HashMap::new();
        }

        let k1 = self.bm25_k1;
        let b = 0.75f32;

        // Average doc length (in unique words) across the corpus — we use the
        // number of unique stems per doc as length, matching what word_doc_tf
        // records. This is a stable proxy for doc length in practice.
        let total_len: usize = self.word_doc_tf.values().map(|m| m.len()).sum();
        let avg_doc_len = (total_len as f32 / self.word_doc_tf.len().max(1) as f32).max(1.0);

        // Score: sum over query tokens of idf(t) * tf_saturation(tf, doc_len)
        // Classic BM25 formula.
        let mut scores: HashMap<String, f32> = HashMap::new();
        for token in &q_tokens {
            let idf = *self.word_idf.get(token).unwrap_or(&0.0);
            if idf <= 0.0 { continue; }
            if let Some(docs) = self.word_inverted_index.get(token) {
                for doc_id in docs {
                    let tf_map = match self.word_doc_tf.get(doc_id) { Some(m) => m, None => continue };
                    let tf = *tf_map.get(token).unwrap_or(&0) as f32;
                    if tf <= 0.0 { continue; }
                    let doc_len = tf_map.len() as f32;
                    let norm = 1.0 - b + b * (doc_len / avg_doc_len);
                    let tf_sat = (tf * (k1 + 1.0)) / (tf + k1 * norm);
                    let contrib = idf * tf_sat;
                    *scores.entry(doc_id.clone()).or_insert(0.0) += contrib;
                }
            }
        }
        scores
    }


    /// Enable persistent storage using memory-mapped files
    /// 
    /// This replaces the in-memory text store with a disk-backed mmap store,
    /// reducing RAM usage dramatically for large document collections.
    /// 
    /// # Arguments
    /// * `base_path` - Path prefix for storage files (e.g., "/data/index" creates "/data/index.texts")
    /// 
    /// # Returns
    /// * `Ok(())` on success
    /// * `Err(String)` if mmap feature is not enabled or file creation fails
    #[cfg(feature = "mmap")]
    pub fn enable_persistence(&mut self, base_path: &str) -> Result<(), String> {
        let text_path = format!("{}.texts", base_path);
        self.text_store = TextStorage::new_persistent(&text_path)
            .map_err(|e| format!("Failed to create persistent text store: {}", e))?;
        Ok(())
    }
    
    /// Enable persistent storage (stub for when mmap feature is disabled)
    #[cfg(not(feature = "mmap"))]
    pub fn enable_persistence(&mut self, _base_path: &str) -> Result<(), String> {
        Err("Persistent storage requires the 'mmap' feature to be enabled".to_string())
    }
    
    /// Open existing persistent storage
    /// 
    /// # Arguments
    /// * `base_path` - Path prefix for storage files
    /// 
    /// # Returns
    /// * `Ok(())` on success, loading existing data
    #[cfg(feature = "mmap")]
    pub fn open_persistent(&mut self, base_path: &str) -> Result<(), String> {
        let text_path = format!("{}.texts", base_path);
        self.text_store = TextStorage::open_persistent(&text_path)
            .map_err(|e| format!("Failed to open persistent text store: {}", e))?;
        Ok(())
    }
    
    /// Open persistent storage (stub for when mmap feature is disabled)
    #[cfg(not(feature = "mmap"))]
    pub fn open_persistent(&mut self, _base_path: &str) -> Result<(), String> {
        Err("Persistent storage requires the 'mmap' feature to be enabled".to_string())
    }
    
    /// Check if using persistent storage
    pub fn is_persistent(&self) -> bool {
        self.text_store.is_persistent()
    }
    
    /// Set tokenizer (matches Python's lazy initialization)
    pub fn set_tokenizer(&mut self, tokenizer: Arc<Tokenizer>) {
        self.tokenizer = Some(tokenizer);
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // BERT TOKENIZATION (matches Python _tokenize, _get_tokens, _get_tokens_with_positions)
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Tokenize text using BERT tokenizer (matches Python _tokenize)
    /// NO TRUNCATION - captures ALL tokens for perfect recall
    fn tokenize(&self, text: &str) -> Vec<u32> {
        #[cfg(feature = "bert")]
        {
            if let Some(ref tokenizer) = self.tokenizer {
                let text_lower = text.to_lowercase();
                match tokenizer.encode(text_lower, false) {
                    Ok(encoding) => return encoding.get_ids().to_vec(),
                    Err(_) => return Vec::new(),
                }
            }
        }
        // Fallback: no tokenizer available
        let _ = text;
        Vec::new()
    }

    /// Get token set (crystal addresses) - matches Python _get_tokens
    fn get_tokens(&self, text: &str) -> HashSet<u32> {
        let ids = self.tokenize(text);
        ids.into_iter()
            .filter(|&tid| tid >= self.min_token_id && !self.special_tokens.contains(&tid))
            .collect()
    }

    /// Get tokens with positions - matches Python _get_tokens_with_positions
    /// Returns: Vec<(token_id, token_string, position)>
    fn get_tokens_with_positions(&self, text: &str) -> Vec<(u32, String, usize)> {
        #[cfg(feature = "bert")]
        if let Some(ref tokenizer) = self.tokenizer {
            let text_lower = text.to_lowercase();
            match tokenizer.encode(text_lower, false) {
                Ok(encoding) => {
                    let ids = encoding.get_ids();
                    let tokens = encoding.get_tokens();

                    return ids.iter()
                        .zip(tokens.iter())
                        .enumerate()
                        .filter_map(|(pos, (&tid, token_str))| {
                            if tid >= self.min_token_id && !self.special_tokens.contains(&tid) {
                                // Clean token string (remove ## prefix for subwords)
                                let clean_str = token_str.replace("##", "").replace("Ġ", "").trim().to_string();
                                if !clean_str.is_empty() {
                                    Some((tid, clean_str, pos))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        })
                        .collect();
                }
                Err(_) => return Vec::new(),
            }
        }
        // Fallback: no tokenizer available
        let _ = text;
        Vec::new()
    }

    /// Get stopword tokens (matches Python _get_stopword_tokens)
    fn get_stopword_tokens(&mut self) -> &HashSet<u32> {
        if self.stopword_tokens.is_none() {
            let mut stopword_set = HashSet::new();
            for word in STOPWORDS {
                let tokens = self.tokenize(word);
                stopword_set.extend(tokens);
            }
            self.stopword_tokens = Some(stopword_set);
        }
        self.stopword_tokens.as_ref().unwrap()
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // SIMPLE WORD TOKENIZATION (for fuzzy matching - NOT BERT!)
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Simple word tokenization (matches Python: re.findall(r'\b\w+\b', text.lower()))
    /// This is CRITICAL for fuzzy matching because BERT breaks words:
    /// - BERT: "anderton" → ["and", "##erton"] ← BROKEN for Soundex
    /// - Simple: "anderton" → ["anderton"] ← WORKS for Soundex
    fn simple_tokenize(&self, text: &str) -> Vec<String> {
        let re = Regex::new(r"\w+").unwrap();
        re.find_iter(&text.to_lowercase())
            .map(|m| {
                let word = m.as_str();
                // Apply Porter2 stemming for morphological normalization
                // "technologies" → "technolog", "running" → "run"
                self.stemmer.stem(word).to_string()
            })
            .collect()
    }

    /// Stem a single word using Porter2 English stemmer.
    fn stem_word(&self, word: &str) -> String {
        self.stemmer.stem(word).to_string()
    }
    
    /// Filter stopwords
    fn filter_stopwords(&self, words: Vec<String>) -> Vec<String> {
        words.into_iter()
            .filter(|w| !STOPWORDS.contains(&w.as_str()))
            .collect()
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // SOUNDEX + LEVENSHTEIN (Fuzzy Matching)
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Generate Soundex code for phonetic matching (Step 1: aligned with sca_dropin).
    /// Maps similar-sounding words to same code (e.g., "anderton" → "A536", "anderson" → "A536").
    /// Empty word returns "0000" to match sca_dropin; first letter's code used as prev to collapse duplicates.
    fn get_soundex(&self, word: &str) -> String {
        Self::get_soundex_static(word)
    }

    /// Soundex (no `&self` — usable from parallel doc preprocessing, #4).
    fn get_soundex_static(word: &str) -> String {
        if word.is_empty() {
            return "0000".to_string();
        }
        let word_upper: Vec<char> = word.to_uppercase().chars().collect();
        let first_letter = word_upper[0];
        let get_code = |c: char| -> Option<char> {
            match c {
                'B' | 'F' | 'P' | 'V' => Some('1'),
                'C' | 'G' | 'J' | 'K' | 'Q' | 'S' | 'X' | 'Z' => Some('2'),
                'D' | 'T' => Some('3'),
                'L' => Some('4'),
                'M' | 'N' => Some('5'),
                'R' => Some('6'),
                _ => None,
            }
        };
        let mut result = String::with_capacity(4);
        result.push(first_letter);
        let mut prev_code: Option<char> = get_code(first_letter);
        for &c in word_upper.iter().skip(1) {
            if let Some(code) = get_code(c) {
                if Some(code) != prev_code {
                    result.push(code);
                    if result.len() == 4 {
                        break;
                    }
                }
                prev_code = Some(code);
            } else {
                prev_code = None;
            }
        }
        while result.len() < 4 {
            result.push('0');
        }
        result
    }
    
    /// Levenshtein distance (edit distance); Step 2: aligned with sca_dropin (char-based).
    /// Used for ranking Soundex candidates by spelling similarity.
    fn levenshtein(&self, a: &str, b: &str) -> usize {
        let a_len = a.chars().count();
        let b_len = b.chars().count();
        if a_len == 0 {
            return b_len;
        }
        if b_len == 0 {
            return a_len;
        }
        let a_chars: Vec<char> = a.chars().collect();
        let b_chars: Vec<char> = b.chars().collect();
        let mut prev_row: Vec<usize> = (0..=b_len).collect();
        let mut curr_row: Vec<usize> = vec![0; b_len + 1];
        for i in 1..=a_len {
            curr_row[0] = i;
            for j in 1..=b_len {
                let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
                curr_row[j] = (prev_row[j] + 1)
                    .min(curr_row[j - 1] + 1)
                    .min(prev_row[j - 1] + cost);
            }
            std::mem::swap(&mut prev_row, &mut curr_row);
        }
        prev_row[b_len]
    }
    
    /// Fuzzy expand a WORD (Step 3: aligned with sca_dropin).
    /// Soundex blocking + Levenshtein ranking; filter dist ≤ 2; fallback to [word_lower] if empty.
    pub fn fuzzy_expand_word(&self, word: &str, top_k: usize) -> Vec<String> {
        let word_lower = word.to_lowercase();

        if self.word_inverted_index.contains_key(&word_lower) {
            return vec![word_lower];
        }

        let soundex_code = self.get_soundex(&word_lower);
        let candidates = match self.word_phonetic_index.get(&soundex_code) {
            Some(c) => c,
            None => return vec![word_lower],
        };

        let mut ranked: Vec<(String, usize)> = candidates
            .iter()
            .map(|c| (c.clone(), self.levenshtein(&word_lower, c)))
            .filter(|(_, dist)| *dist <= 2)
            .collect();

        ranked.sort_by_key(|(_, dist)| *dist);

        let result: Vec<String> = ranked.into_iter().take(top_k).map(|(w, _)| w).collect();

        if result.is_empty() {
            vec![word_lower]
        } else {
            result
        }
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // CODE DETECTION (Step 5: aligned with sca_dropin)
    // ═══════════════════════════════════════════════════════════════════════════

    /// Code detection: passkeys/codes (5–10 pure digits).
    /// Excludes dates, currency, ordinals, measurements. Matches sca_dropin exactly.
    fn looks_like_code(&self, word: &str) -> bool {
        if word.contains('/') || word.contains('-') {
            return false;
        }
        if word.contains('$') || word.contains('€') || word.contains('£') || word.contains(',') {
            return false;
        }
        if word.starts_with('(') || word.starts_with('[') {
            return false;
        }
        if word.chars().any(|c| c == '±' || c == '×' || c == '÷') {
            return false;
        }
        let clean: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
        if clean.len() < 5 || clean.len() > 15 {
            return false;
        }
        let lower = clean.to_lowercase();
        if lower.ends_with("st") || lower.ends_with("nd") || lower.ends_with("rd") || lower.ends_with("th") {
            let prefix = &lower[..lower.len() - 2];
            if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
        }
        let measurement_suffixes = ["kg", "km", "cm", "mm", "ml", "mg", "gb", "mb", "kb", "hz", "bn", "mn", "bln"];
        for suffix in measurement_suffixes {
            if lower.ends_with(suffix) {
                let prefix = &lower[..lower.len() - suffix.len()];
                if prefix.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    return false;
                }
            }
        }
        let chars: Vec<char> = clean.chars().collect();
        let digit_count = chars.iter().filter(|c| c.is_ascii_digit()).count();
        let letter_count = chars.iter().filter(|c| c.is_ascii_alphabetic()).count();
        if letter_count == 0 && digit_count >= 5 && digit_count <= 10 {
            return true;
        }
        false
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // QUERY ANALYSIS & ROUTING (Step 6: sca_dropin analyze_query + route → API)
    // ═══════════════════════════════════════════════════════════════════════════

    /// Analyze query like sca_dropin: q_words, q_expanded, idf_avg, oov_ratio, has_code, has_typo, route.
    fn analyze_query(&self, query_text: &str) -> (QueryRoute, Vec<String>, HashSet<String>, f32, f32, bool, bool) {
        let q_words: Vec<String> = query_text
            .split_whitespace()
            .map(|s| {
                let lower = s.to_lowercase();
                lower.trim_end_matches(|c: char| c.is_ascii_punctuation()).to_string()
            })
            .filter(|s| s.len() >= 3)
            .collect();

        if q_words.is_empty() {
            return (QueryRoute::PureSemantic, q_words, HashSet::new(), 0.5, 0.0, false, false);
        }

        let mut has_any_code = false;
        let mut has_typo = false;
        let mut known_word_count = 0;
        let mut total_idf = 0.0f32;
        let mut oov_count = 0;
        let mut q_expanded: HashSet<String> = HashSet::new();

        for word in &q_words {
            if self.looks_like_code(word) {
                has_any_code = true;
                total_idf += 5.0;
                known_word_count += 1;
                q_expanded.insert(word.clone());
                continue;
            }
            // Try both raw word and stemmed form for vocabulary lookup
            let stemmed = self.stem_word(word);
            let in_idf = self.word_idf.get(word).or_else(|| self.word_idf.get(&stemmed));
            let in_vocab = self.word_vocabulary.contains(word) || self.word_vocabulary.contains(&stemmed);
            if in_idf.is_some() || in_vocab {
                known_word_count += 1;
                let idf = *in_idf.unwrap_or(&1.5);
                total_idf += idf;
                // Add both raw and stemmed forms for maximum recall
                q_expanded.insert(word.clone());
                if stemmed != *word {
                    q_expanded.insert(stemmed);
                }
            } else {
                oov_count += 1;
                // Try compound word splitting for code-intent words
                // e.g. "passkey" → ["pass", "key"] when doc has "pass key"
                let mut compound_found = false;
                for &(compound, parts) in COMPOUND_SPLITS {
                    if word == compound {
                        for &part in parts {
                            if part.len() >= 3 {
                                q_expanded.insert(part.to_string());
                            }
                        }
                        q_expanded.insert(word.clone()); // keep original too
                        compound_found = true;
                        break;
                    }
                }
                if !compound_found {
                    let fuzzy_matches = self.fuzzy_expand_word(word, 5);
                    let valid_matches: Vec<String> = fuzzy_matches
                        .iter()
                        .filter(|m| *m != word && self.word_vocabulary.contains(*m))
                        .cloned()
                        .collect();
                    if !valid_matches.is_empty() {
                        has_typo = true;
                        for m in valid_matches {
                            q_expanded.insert(m);
                        }
                    } else {
                        q_expanded.insert(word.clone());
                    }
                }
            }
        }

        let content_words: Vec<f32> = q_expanded
            .iter()
            .filter_map(|w| self.word_idf.get(w).copied())
            .filter(|&idf| idf >= 1.5)
            .collect();
        let idf_avg = if !content_words.is_empty() {
            content_words.iter().sum::<f32>() / content_words.len() as f32
        } else if has_any_code {
            10.0
        } else if known_word_count > 0 {
            total_idf / known_word_count as f32
        } else {
            1.0
        };

        let non_code_count = q_words.iter().filter(|w| !self.looks_like_code(w)).count();
        let oov_ratio = if non_code_count > 0 {
            oov_count as f32 / non_code_count as f32
        } else {
            0.0
        };

        let has_code_intent = q_words.iter().any(|w| CODE_INTENT_WORDS.contains(&w.as_str()));
        let high_idf_count = q_expanded
            .iter()
            .filter(|w| *self.word_idf.get(*w).unwrap_or(&0.0) > 2.5)
            .count();
        let is_short_discourse = q_words.len() <= 8
            && !has_any_code
            && !has_code_intent
            && idf_avg <= 1.2
            && high_idf_count == 0
            && oov_ratio < 0.1;

        let route = if has_any_code || has_code_intent {
            QueryRoute::PureLexical
        } else if is_short_discourse {
            QueryRoute::PureSemantic
        } else {
            QueryRoute::FullHybrid
        };

        (route, q_words, q_expanded, idf_avg, oov_ratio, has_any_code, has_typo)
    }

    /// Public API: query type string for callers (engine.rs, recall.rs). The three
    /// SCA-native routes — "high_lexical" (code/NIAH), "pure_semantic" (short
    /// conversational paraphrase), "balanced" (full hybrid). Recall layers gate
    /// BM25/grep signals off this to avoid hurting paraphrase queries.
    pub fn detect_query_type(&self, query: &str) -> &'static str {
        if query.is_empty() || query.trim().is_empty() {
            return "balanced";
        }
        let (route, _, _, _, _, _, _) = self.analyze_query(query);
        match route {
            QueryRoute::PureLexical => "high_lexical",
            QueryRoute::PureSemantic => "pure_semantic",
            QueryRoute::FullHybrid => "balanced",
        }
    }
    
    /// Get alpha value for hybrid scoring (SIMPLIFIED for IDF-Surprise).
    /// 
    /// With IDF-weighting, token scoring is smarter:
    /// - Rare tokens (like "Bobolink") get HIGH IDF weight
    /// - Common tokens (like "the") get LOW IDF weight
    /// 
    /// This means alpha=0.70 works well for BOTH lexical AND semantic queries!
    /// Only 3 distinct modes needed:
    /// 
    /// - high_lexical (0.85): NIAH/Passkey - exact token match critical
    /// - balanced (0.70): Default - IDF-Surprise handles all semantic queries
    /// - pure_semantic (0.00): Paraphrase detection - meaning > words
    fn get_alpha(&self, query_type: &str) -> f32 {
        match query_type {
            "high_lexical" => 0.85,
            "balanced" => 0.70,
            "high_semantic" => 0.70,     // MAPPED → balanced (IDF handles this)
            "very_high_semantic" => 0.70, // MAPPED → balanced (IDF handles this)
            "pure_semantic" => 0.0,
            _ => 0.70,
        }
    }
    
    /// Extract identifier-like tokens from a query for exact match verification.
    fn extract_identifiers(&self, query: &str) -> Vec<String> {
        let mut identifiers = Vec::new();
        let punct_chars: &[char] = &['.', ',', ';', ':', '!', '?', '(', ')', '[', ']', '{', '}', '"', '\''];
        
        for word in query.split_whitespace() {
            let word_clean: String = word.trim_matches(punct_chars).to_string();
            
            let word_upper = word_clean.chars().filter(|c| c.is_uppercase()).count();
            let word_digit = word_clean.chars().filter(|c| c.is_ascii_digit()).count();
            let word_alpha = word_clean.chars().filter(|c| c.is_alphabetic()).count();
            let word_alnum = word_alpha + word_digit;
            
            if word_alnum == 0 {
                continue;
            }
            
            let word_upper_ratio = word_upper as f32 / word_alnum as f32;
            let word_digit_ratio = word_digit as f32 / word_alnum as f32;
            
            // Identifier criteria (matching Python)
            let is_identifier = word_clean.len() >= 5 && (
                word_upper_ratio > 0.5 ||
                (word_digit_ratio > 0.1 && word_upper_ratio > 0.2) ||
                word_digit_ratio > 0.3 ||
                (word_clean.contains('-') && word_clean.len() >= 7) ||
                (word_clean.contains('_') && word_clean.len() >= 7)
            );
            
            // Also include pure numeric codes (passkeys like "84729")
            let is_numeric_code = word_clean.len() >= 4 && word_digit_ratio > 0.8;
            
            if is_identifier || is_numeric_code {
                identifiers.push(word_clean);
            }
        }
        
        identifiers
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // IDF CACHE MANAGEMENT
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Rebuild IDF cache. Token IDF and word IDF use same formula as sca_dropin/Python (Step 7).
    /// Word IDF: log((N+1)/(df+1))+1, with normalized word keys (lowercase, trim trailing punct) and df = doc count.
    pub fn rebuild_idf(&mut self) {
        if !self.idf_dirty || self.doc_count == 0 {
            return;
        }

        let n = self.doc_count as f32;

        // Token-level IDF (unchanged)
        self.idf_cache.clear();
        for (token_id, doc_ids) in self.inverted_index.items() {
            let df = doc_ids.len() as f32;
            let idf = ((n + 1.0) / (df + 1.0)).ln() + 1.0;
            self.idf_cache.insert(token_id, idf);
        }

        // Word-level IDF and PHONETIC INDEX (Step 7: same formula and normalized word set as sca_dropin)
        self.word_idf.clear();
        self.word_phonetic_index.clear();

        // Build norm -> doc_ids so we merge raw words that normalize to the same form (same shape as sca_dropin)
        let mut norm_to_docs: HashMap<String, HashSet<String>> = HashMap::new();
        for word in &self.word_vocabulary {
            let w_normalized: String = word
                .to_lowercase()
                .trim_end_matches(|c: char| c.is_ascii_punctuation())
                .to_string();
            if w_normalized.len() < 3 {
                continue;
            }
            if let Some(doc_ids) = self.word_inverted_index.get(word) {
                let set = norm_to_docs.entry(w_normalized.clone()).or_insert_with(HashSet::new);
                for doc_id in doc_ids {
                    set.insert(doc_id.clone());
                }
            }
        }

        for (w_norm, doc_set) in &norm_to_docs {
            let df = doc_set.len() as f32;
            let idf = ((n + 1.0) / (df + 1.0)).ln() + 1.0;
            self.word_idf.insert(w_norm.clone(), idf);

            let sx = self.get_soundex(w_norm);
            self.word_phonetic_index
                .entry(sx)
                .or_insert_with(HashSet::new)
                .insert(w_norm.clone());
        }

        self.idf_dirty = false;
    }
    
    /// Get IDF for token. Returns 0.5 for unknown tokens.
    fn get_idf(&self, token_id: u32) -> f32 {
        *self.idf_cache.get(&token_id).unwrap_or(&0.5)
    }
    
    /// Get bigrams from sorted token set (for phrase matching).
    fn get_bigrams(&self, tokens: &HashSet<u32>) -> HashSet<(u32, u32)> {
        let mut sorted_tokens: Vec<u32> = tokens.iter().cloned().collect();
        sorted_tokens.sort();
        
        if sorted_tokens.len() < 2 {
            return HashSet::new();
        }
        
        sorted_tokens.windows(2)
            .map(|w| (w[0], w[1]))
            .collect()
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // INDEXING: Stream documents into crystal lattice
    // Matches Python _crystalline.py index() EXACTLY
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Index a document with BERT tokens and POSITIONS (matches Python index())
    /// 
    /// This is a 1:1 match with Python _crystalline.py:
    /// 1. Uses BERT tokenization for the inverted index (ART)
    /// 2. Uses simple word tokenization for fuzzy matching (Soundex/Levenshtein)
    pub fn index(&mut self, doc_id: &str, text: &str) {
        // Get BERT tokens WITH POSITIONS (matches Python _get_tokens_with_positions)
        let tokens_with_pos = self.get_tokens_with_positions(text);
        
        // PHASE 2: Use RoaringBitmap instead of HashSet<u32> - ~100x smaller
        let mut token_set = RoaringBitmap::new();
        for (tid, _, _) in &tokens_with_pos {
            token_set.insert(*tid);
        }
        
        // Remove old inverted index entries if re-indexing (matches Python)
        if let Some(old_tokens) = self.doc_token_sets.get(doc_id) {
            // RoaringBitmap iteration
            for token_id in old_tokens.iter() {
                self.inverted_index.discard(token_id, doc_id);
            }
        }
        
        // Store in FORWARD index (doc → tokens) - matches Python
        if !self.doc_ids.contains(&doc_id.to_string()) {
            self.doc_ids.push(doc_id.to_string());
        }
        
        // Store text in text_store (CLEANUP: removed legacy doc_texts HashMap)
        if !self.doc_id_to_idx.contains_key(doc_id) {
            let idx = self.text_store.append_text(text).unwrap_or(0);
            self.doc_id_to_idx.insert(doc_id.to_string(), idx);
        } else {
            // Re-indexing: update existing entry
            let idx = self.text_store.append_text(text).unwrap_or(0);
            self.doc_id_to_idx.insert(doc_id.to_string(), idx);
        }
        
        // PHASE 2: Insert RoaringBitmap (compressed token set)
        self.doc_token_sets.insert(doc_id.to_string(), token_set);
        self.doc_count = self.doc_ids.len();
        
        // Store token counts for TF-IDF (matches Python)
        let mut token_counts: HashMap<u32, u32> = HashMap::new();
        for (tid, _, _) in &tokens_with_pos {
            *token_counts.entry(*tid).or_insert(0) += 1;
        }
        self.doc_token_counts.insert(doc_id.to_string(), token_counts);
        
        // Store in INVERTED index WITH POSITIONS - O(1) ART insertion (matches Python)
        // Store in POSITION index (doc → pos → token_id)
        let mut position_map: HashMap<usize, u32> = HashMap::new();
        
        for (token_id, token_str, pos) in &tokens_with_pos {
            self.inverted_index.add(*token_id, doc_id, Some(*pos));
            position_map.insert(*pos, *token_id);
            self.token_strings.insert(*token_id, token_str.clone());
        }
        self.position_tokens.insert(doc_id.to_string(), position_map);
        
        // ═══════════════════════════════════════════════════════════════
        // WORD-LEVEL INDEX (for fuzzy matching with simple tokenization)
        // This is CRITICAL because BERT sub-tokenization breaks Soundex!
        // Matches Python: words = self._simple_tokenize(text)
        // ═══════════════════════════════════════════════════════════════
        let words = self.simple_tokenize(text);
        let mut word_counts: HashMap<String, u32> = HashMap::new();
        
        for word in &words {
            *word_counts.entry(word.clone()).or_insert(0) += 1;
        }
        
        self.word_doc_tf.insert(doc_id.to_string(), word_counts.clone());
        
        for word in word_counts.keys() {
            self.word_vocabulary.insert(word.clone());
            self.word_inverted_index
                .entry(word.clone())
                .or_insert_with(HashSet::new)
                .insert(doc_id.to_string());
        }
        
        // Mark IDF cache as dirty (needs rebuild on next search)
        self.idf_dirty = true;
    }
    
    /// Index multiple documents.
    pub fn index_many(&mut self, documents: Vec<(&str, &str)>) -> HashMap<String, usize> {
        let mut total_tokens = 0;
        for (doc_id, text) in &documents {
            self.index(doc_id, text);
            total_tokens += self.word_doc_tf.get(*doc_id).map(|m| m.len()).unwrap_or(0);
        }
        
        let mut result = HashMap::new();
        result.insert("num_docs".to_string(), documents.len());
        result.insert("total_tokens".to_string(), total_tokens);
        result
    }
    
    /// Stream index a very long document (2M+ tokens).
    /// Processes in chunks to maintain constant memory.
    /// Matches Python _crystalline.py stream_index() EXACTLY.
    pub fn stream_index(&mut self, doc_id: &str, text: &str, chunk_size: usize) -> HashMap<String, usize> {
        // Remove old entries if re-indexing (matches Python)
        // PHASE 2: RoaringBitmap iteration
        if let Some(old_tokens) = self.doc_token_sets.get(doc_id) {
            for token_id in old_tokens.iter() {
                self.inverted_index.discard(token_id, doc_id);
            }
        }
        
        if !self.doc_ids.contains(&doc_id.to_string()) {
            self.doc_ids.push(doc_id.to_string());
        }
        
        // Store text in text_store (CLEANUP: removed legacy doc_texts HashMap)
        if !self.doc_id_to_idx.contains_key(doc_id) {
            let idx = self.text_store.append_text(text).unwrap_or(0);
            self.doc_id_to_idx.insert(doc_id.to_string(), idx);
        } else {
            // Re-indexing: update existing entry
            let idx = self.text_store.append_text(text).unwrap_or(0);
            self.doc_id_to_idx.insert(doc_id.to_string(), idx);
        }
        
        // PHASE 2: Use RoaringBitmap for compressed token set
        self.doc_token_sets.insert(doc_id.to_string(), RoaringBitmap::new());
        
        let mut chunks = 0;
        let mut pos = 0;
        
        // Process text in chunks (matches Python)
        while pos < text.len() {
            let mut end = (pos + chunk_size).min(text.len());
            // Ensure end is on a UTF-8 char boundary
            while end < text.len() && !text.is_char_boundary(end) {
                end += 1;
            }
            let chunk = &text[pos..end];
            
            // Get BERT tokens for this chunk (matches Python: chunk_tokens = self._get_tokens(chunk))
            let chunk_tokens = self.get_tokens(chunk);
            
            // Update forward index - PHASE 2: RoaringBitmap extend
            if let Some(token_set) = self.doc_token_sets.get_mut(doc_id) {
                token_set.extend(chunk_tokens.iter().copied());
            }
            
            // Update inverted index - O(1) ART insertion per token (matches Python)
            for &token_id in &chunk_tokens {
                self.inverted_index.add(token_id, doc_id, None);
            }
            
            chunks += 1;
            pos = end;
        }
        
        // PHASE 2: RoaringBitmap.len() returns u64, cast to usize
        let token_count = self.doc_token_sets.get(doc_id).map(|s| s.len() as usize).unwrap_or(0);
        
        let mut result = HashMap::new();
        result.insert("doc_id".to_string(), 1);
        result.insert("chunks".to_string(), chunks);
        result.insert("tokens".to_string(), token_count);
        
        result
    }
    
    /// Set embedding for a document
    pub fn set_embedding(&mut self, doc_id: &str, embedding: Vec<f32>) {
        self.doc_embeddings.insert(doc_id.to_string(), embedding);
    }

    /// Set passage embeddings for a document (legacy LongEmbed MaxSim).
    pub fn set_passage_embeddings(&mut self, doc_id: &str, passage_embeddings: Vec<Vec<f32>>) {
        if passage_embeddings.is_empty() {
            self.doc_passage_embeddings.remove(doc_id);
        } else {
            self.doc_passage_embeddings
                .insert(doc_id.to_string(), passage_embeddings);
        }
    }
    
    /// Get embedding for a document
    pub fn get_embedding(&self, doc_id: &str) -> Option<&Vec<f32>> {
        self.doc_embeddings.get(doc_id)
    }

    /// Legacy: split long text into overlapping passages (Python `_crystalline.pyx::get_passages`).
    pub fn legacy_get_passages(&self, text: &str, chunk_size: usize, stride: usize, max_chars: usize) -> Vec<String> {
        // Strip HTML/XML tags (best-effort, matches Python behavior closely)
        let re = Regex::new(r"<[^>]+>").ok();
        let mut cleaned = if let Some(re) = re {
            re.replace_all(text, " ").to_string()
        } else {
            text.to_string()
        };

        // Sample strategically for very long docs (beginning, early-mid, late-mid, end)
        if cleaned.len() > max_chars {
            let quarter = max_chars / 4;
            let len = cleaned.len();
            let b = &cleaned[0..quarter.min(len)];
            let em_start = (len / 4).min(len);
            let em_end = (em_start + quarter).min(len);
            let early_mid = &cleaned[em_start..em_end];
            let lm_center = (len * 3 / 4).min(len);
            let lm_start = lm_center.saturating_sub(quarter / 2);
            let lm_end = (lm_center + quarter / 2).min(len);
            let late_mid = &cleaned[lm_start..lm_end];
            let end_start = len.saturating_sub(quarter);
            let end = &cleaned[end_start..len];
            cleaned = format!("{} {} {} {}", b, early_mid, late_mid, end);
        }

        let mut passages = Vec::new();
        if stride == 0 || chunk_size == 0 {
            return vec![cleaned.chars().take(max_chars.min(2000)).collect()];
        }

        let bytes = cleaned.as_bytes();
        let mut start = 0usize;
        while start < bytes.len() {
            let end = (start + chunk_size).min(bytes.len());
            // Safety: this may cut UTF-8 in the middle; LongEmbed corpora are mostly ASCII/Latin,
            // and this mirrors the simple Python slicing behavior closely.
            let chunk = String::from_utf8_lossy(&bytes[start..end]).trim().to_string();
            if chunk.len() >= 100 {
                passages.push(chunk);
            }
            start = start.saturating_add(stride);
        }

        if passages.is_empty() {
            let fallback_end = chunk_size.min(bytes.len());
            let fallback = String::from_utf8_lossy(&bytes[0..fallback_end]).to_string();
            return vec![fallback];
        }

        passages
    }

    /// Legacy LongEmbed search (Step 9: sca_dropin-style passage MaxSim + lexical blend).
    /// Passage MaxSim (max dot over passage embeddings) + word-level IDF overlap + exact-match safety; dynamic alpha blend.
    pub fn search_legacy(
        &mut self,
        query: &str,
        top_k: usize,
        query_embedding: Option<&[f32]>,
        alpha_override: Option<f32>,
    ) -> Vec<(String, f32)> {
        if self.doc_ids.is_empty() {
            return vec![];
        }
        if self.idf_dirty {
            self.rebuild_idf();
        }

        let (_route, _q_words, q_expanded, _idf_avg, _oov, _has_code, _has_typo) = self.analyze_query(query);
        let query_lower = query.to_lowercase();

        // Candidates from word-level index (like sca_dropin); fallback to all docs
        let candidate_docs: HashSet<String> = {
            let mut cand = HashSet::new();
            for word in &q_expanded {
                if let Some(docs) = self.word_inverted_index.get(word) {
                    cand.extend(docs.iter().cloned());
                }
            }
            if cand.is_empty() {
                self.doc_ids.iter().cloned().collect()
            } else {
                cand
            }
        };

        let mut scores: Vec<(String, f32)> = Vec::new();
        for doc_id in &candidate_docs {
            let doc_text = match self.get_text(doc_id) {
                Some(t) => t,
                None => continue,
            };
            let doc_text_lower = doc_text.to_lowercase();
            if doc_text_lower.contains(&query_lower) {
                scores.push((doc_id.clone(), 1.0));
                continue;
            }

            let doc_words: HashSet<String> = self
                .word_doc_tf
                .get(doc_id)
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default();
            let overlap: HashSet<String> = q_expanded.intersection(&doc_words).cloned().collect();

            // Lexical: IDF overlap ratio (sca_dropin-style)
            let mut idf_matched = 0.0f32;
            let mut idf_total = 0.0f32;
            for word in &q_expanded {
                let idf = *self.word_idf.get(word).unwrap_or(&1.0);
                idf_total += idf;
                if doc_words.contains(word) {
                    idf_matched += idf;
                }
            }
            let s_lex = if idf_total > 0.0 {
                idf_matched / idf_total
            } else {
                0.0
            };

            // Semantic: passage MaxSim (max dot over passages) or doc embedding fallback
            let s_sem: f32 = if let Some(q_emb) = query_embedding {
                if let Some(passages) = self.doc_passage_embeddings.get(doc_id) {
                    passages
                        .iter()
                        .map(|p_emb| dot_product(q_emb, p_emb).max(0.0))
                        .fold(0.0f32, f32::max)
                } else if let Some(d_emb) = self.doc_embeddings.get(doc_id) {
                    dot_product(q_emb, d_emb).max(0.0)
                } else {
                    0.0
                }
            } else {
                0.0
            };

            // Dynamic alpha from IDF overlap (mirror sca_dropin full_hybrid)
            let base_alpha = if overlap.is_empty() {
                1.0
            } else {
                let idf_overlap: f32 = overlap.iter().map(|w| *self.word_idf.get(w).unwrap_or(&0.5)).sum::<f32>()
                    / overlap.len() as f32;
                let idf_query: f32 = q_expanded
                    .iter()
                    .map(|w| *self.word_idf.get(w).unwrap_or(&0.5))
                    .sum::<f32>()
                    / q_expanded.len().max(1) as f32;
                (1.0 - idf_overlap / idf_query.max(0.001)).clamp(0.0, 1.0)
            };
            let blend_alpha = alpha_override.unwrap_or(base_alpha).clamp(0.0, 1.0);
            let final_score = blend_alpha * s_sem + (1.0 - blend_alpha) * s_lex;
            if final_score > 0.0 {
                scores.push((doc_id.clone(), final_score));
            }
        }

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_k);
        scores
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // SEARCH: Unified auto-routing search
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// UNIFIED SEARCH (Step 8: sca_dropin-style route + candidate flow + scoring)
    /// Route: PureLexical → keyword overlap ratio; PureSemantic → dot-product only; FullHybrid → IDF overlap + TF-IDF 70/30 + length norm + phrase boost + alpha blend.
    pub fn search(
        &mut self,
        query: &str,
        top_k: usize,
        query_embedding: Option<&[f32]>,
        alpha_override: Option<f32>,
    ) -> Vec<(String, f32)> {
        if self.doc_ids.is_empty() {
            return vec![];
        }
        if self.idf_dirty {
            self.rebuild_idf();
        }

        let (route, q_words, q_expanded, _idf_avg, _oov, _has_code, _has_typo) = self.analyze_query(query);
        let _alpha = alpha_override.unwrap_or_else(|| {
            self.get_alpha(match route {
                QueryRoute::PureLexical => "high_lexical",
                QueryRoute::PureSemantic => "pure_semantic",
                QueryRoute::FullHybrid => "balanced",
            })
        });
        let query_lower = query.to_lowercase();

        // PATH A: Pure lexical (sca_dropin search_pure_lexical style)
        if route == QueryRoute::PureLexical {
            if q_words.is_empty() {
                return vec![];
            }
            // Use q_expanded (includes compound splits) and IDF-weight hits
            // so rare words like "nathan", "munoz" dominate over stopwords
            let mut results: Vec<(String, f32)> = Vec::new();
            let total_query_idf: f32 = q_expanded.iter()
                .map(|w| *self.word_idf.get(w).unwrap_or(&1.0))
                .sum::<f32>()
                .max(1.0);
            for doc_id in &self.doc_ids {
                let doc_words: HashSet<String> = self
                    .word_doc_tf
                    .get(doc_id)
                    .map(|m| m.keys().cloned().collect())
                    .unwrap_or_default();
                let hit_idf: f32 = q_expanded.iter()
                    .filter(|w| doc_words.contains(w.as_str()))
                    .map(|w| *self.word_idf.get(w).unwrap_or(&1.0))
                    .sum();
                if hit_idf > 0.0 {
                    results.push((doc_id.clone(), hit_idf / total_query_idf));
                }
            }
            results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            return results.into_iter().take(top_k).collect();
        }

        // PATH B: Pure semantic (dot-product only; no lexical)
        if route == QueryRoute::PureSemantic {
            if let Some(q_emb) = query_embedding {
                let mut results: Vec<(String, f32)> = self
                    .doc_ids
                    .iter()
                    .filter_map(|doc_id| {
                        self.doc_embeddings.get(doc_id).map(|d_emb| {
                            (doc_id.clone(), dot_product(q_emb, d_emb).max(0.0))
                        })
                    })
                    .collect();
                results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                return results.into_iter().take(top_k).collect();
            }
            // No embedding: fall through to hybrid with all docs
        }

        // PATH C: Full hybrid (sca_dropin search_full_hybrid-style scoring)
        let candidate_docs: HashSet<String> = {
            let mut cand = HashSet::new();
            for word in &q_expanded {
                if let Some(docs) = self.word_inverted_index.get(word) {
                    cand.extend(docs.iter().cloned());
                }
            }
            if cand.is_empty() {
                self.doc_ids.iter().cloned().collect()
            } else {
                cand
            }
        };
        if candidate_docs.is_empty() {
            return vec![];
        }

        let rare_words: HashSet<String> = q_expanded
            .iter()
            .filter(|w| *self.word_idf.get(*w).unwrap_or(&0.0) > 2.5)
            .cloned()
            .collect();
        let b_param = 0.35f32;
        let total_len: usize = candidate_docs
            .iter()
            .filter_map(|id| self.word_doc_tf.get(id).map(|m| m.len()))
            .sum();
        let avg_doc_len = (total_len as f32 / candidate_docs.len().max(1) as f32).max(1.0);
        let mut scores: Vec<(String, f32)> = Vec::new();

        for doc_id in &candidate_docs {
            let doc_text = match self.get_text(doc_id) {
                Some(t) => t,
                None => continue,
            };
            let doc_tf = match self.word_doc_tf.get(doc_id) {
                Some(t) => t,
                None => continue,
            };
            let doc_words: HashSet<String> = doc_tf.keys().cloned().collect();

            if doc_text.contains(&query_lower) {
                scores.push((doc_id.clone(), 1.0));
                continue;
            }

            let overlap: HashSet<String> = q_expanded.intersection(&doc_words).cloned().collect();
            let idf_overlap: f32 = overlap
                .iter()
                .map(|w| *self.word_idf.get(w).unwrap_or(&0.5))
                .sum::<f32>()
                / overlap.len().max(1) as f32;
            let idf_query: f32 = q_expanded
                .iter()
                .map(|w| *self.word_idf.get(w).unwrap_or(&0.5))
                .sum::<f32>()
                / q_expanded.len().max(1) as f32;
            let base_alpha = (1.0 - idf_overlap / idf_query.max(0.001f32)).clamp(0.0, 1.0);

            let mut entity_score = 0.0f32;
            let mut common_score = 0.0f32;
            let doc_len = doc_words.len() as f32;
            let length_norm = (1.0 - b_param + b_param * (doc_len / avg_doc_len)).max(0.5);
            let k1 = self.bm25_k1;
            for word in &overlap {
                let tf = *doc_tf.get(word).unwrap_or(&1) as f32;
                let idf = *self.word_idf.get(word).unwrap_or(&0.5);
                // Proper BM25 TF saturation: tf*(k1+1)/(tf+k1*length_norm)
                let tf_saturated = (tf * (k1 + 1.0)) / (tf + k1 * length_norm);
                let tfidf = tf_saturated * idf;
                if rare_words.contains(word) {
                    entity_score += tfidf;
                } else {
                    common_score += tfidf;
                }
            }
            let total_tfidf = 0.7 * entity_score + 0.3 * common_score;
            let token_ratio = (total_tfidf / 10.0).min(1.0);
            let boost = 1.0 + token_ratio.powi(2);
            let s_lex_raw = token_ratio * boost;
            let s_lex = s_lex_raw;

            let query_words_raw: Vec<&str> = query.split_whitespace().collect();
            let mut phrase_match_boost = 1.0f32;
            if query_words_raw.len() >= 3 {
                'phrase: for &w_size in &[4_usize, 3] {
                    if query_words_raw.len() < w_size {
                        continue;
                    }
                    for window in query_words_raw.windows(w_size) {
                        let window_clean: Vec<String> = window
                            .iter()
                            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
                            .collect();
                        if !window_clean.iter().all(|w| doc_words.contains(w)) {
                            continue;
                        }
                        let phrase_idf_sum: f32 = window_clean
                            .iter()
                            .map(|w| *self.word_idf.get(w).unwrap_or(&0.5))
                            .sum();
                        if phrase_idf_sum / (w_size as f32) < 2.2 {
                            continue;
                        }
                        let phrase = window.join(" ").to_lowercase();
                        if phrase.trim().len() >= 10 && doc_text.contains(phrase.trim()) {
                            phrase_match_boost = if w_size == 4 { 1.25 } else { 1.15 };
                            break 'phrase;
                        }
                    }
                }
            }
            let s_lex_boosted = s_lex * phrase_match_boost;

            // Semantic: passage MaxSim when available (Step 10 – align with sca_dropin passage-level)
            let s_sem = if let Some(q_emb) = query_embedding {
                if let Some(passages) = self.doc_passage_embeddings.get(doc_id) {
                    passages
                        .iter()
                        .map(|p_emb| dot_product(q_emb, p_emb).max(0.0))
                        .fold(0.0f32, f32::max)
                } else if let Some(d_emb) = self.doc_embeddings.get(doc_id) {
                    dot_product(q_emb, d_emb).max(0.0)
                } else {
                    0.0
                }
            } else {
                0.0
            };
            let final_score = base_alpha * s_sem + (1.0 - base_alpha) * s_lex_boosted;
            if final_score > 0.0 {
                scores.push((doc_id.clone(), final_score));
            }
        }

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_k);
        scores
    }
    
    /// Perfect recall search (exact string match with token filter).
    /// Matches Python _crystalline.py search_exact() EXACTLY.
    /// Two-stage:
    /// 1. Token filter (fast rejection using BERT tokens)
    /// 2. Exact substring verification
    pub fn search_exact(&self, query: &str) -> Vec<(String, f32)> {
        // Get BERT tokens (matches Python: q_tokens = self._get_tokens(query))
        let q_tokens = self.get_tokens(query);
        if q_tokens.is_empty() {
            return Vec::new();
        }
        
        let query_lower = query.to_lowercase();
        let mut matches = Vec::new();
        
        // 🚀 INVERTED INDEX OPTIMIZATION: Get candidate docs from ART (matches Python)
        let mut candidate_docs: HashSet<String> = HashSet::new();
        for token_id in &q_tokens {
            let docs = self.inverted_index.get(*token_id);
            candidate_docs.extend(docs);
        }
        
        // If no candidates from tokens, search all (matches Python)
        if candidate_docs.is_empty() {
            candidate_docs = self.doc_ids.iter().cloned().collect();
        }
        
        for doc_id in candidate_docs {
            // PHASE 1b: Read from text_store instead of doc_texts HashMap
            if let Some(doc_text) = self.get_text(&doc_id) {
                if doc_text.contains(&query_lower) {
                    matches.push((doc_id, 1.0));
                }
            }
        }
        
        matches
    }
    
    /// Find ALL instances (count) of query in each document.
    /// Matches Python _crystalline.py search_all_instances() EXACTLY.
    pub fn search_all_instances(&self, query: &str) -> Vec<(String, usize)> {
        // Get BERT tokens (matches Python: q_tokens = self._get_tokens(query))
        let q_tokens = self.get_tokens(query);
        if q_tokens.is_empty() {
            return Vec::new();
        }
        
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        
        // 🚀 INVERTED INDEX OPTIMIZATION: Get candidate docs from ART (matches Python)
        let mut candidate_docs: HashSet<String> = HashSet::new();
        for token_id in &q_tokens {
            let docs = self.inverted_index.get(*token_id);
            candidate_docs.extend(docs);
        }
        
        if candidate_docs.is_empty() {
            candidate_docs = self.doc_ids.iter().cloned().collect();
        }
        
        for doc_id in candidate_docs {
            // PHASE 1b: Read from text_store instead of doc_texts HashMap
            if let Some(doc_text) = self.get_text(&doc_id) {
                let count = doc_text.to_lowercase().matches(&query_lower).count();
                if count > 0 {
                    results.push((doc_id, count));
                }
            }
        }
        
        results
    }
    
    /// PURE ART Extraction - IDF-based dynamic separator detection.
    /// Matches Python _crystalline.py search_kv() EXACTLY.
    /// 
    /// Uses ART: Find key → Skip common tokens (high doc_freq) → Return rare token (value)
    /// 
    /// NO HARDCODED SEPARATORS - uses document frequency mathematically:
    /// - Common tokens (>10% of docs): "is", ":", "=" → skip
    /// - Rare tokens (<10% of docs): "SECRET_KEY_123" → value!
    pub fn search_kv(&self, key: &str, top_k: i32) -> Vec<(String, String)> {
        let mut results = Vec::new();
        
        // Get BERT tokens with positions (matches Python: key_tokens_with_pos = self._get_tokens_with_positions(key))
        let key_tokens_with_pos = self.get_tokens_with_positions(key);
        if key_tokens_with_pos.is_empty() {
            return results;
        }
        
        // Get candidate docs where ALL key tokens appear (matches Python)
        let key_token_ids: Vec<u32> = key_tokens_with_pos.iter().map(|(tid, _, _)| *tid).collect();
        let mut candidate_docs: Option<HashSet<String>> = None;
        
        for token_id in &key_token_ids {
            let docs = self.inverted_index.get(*token_id);
            if let Some(ref mut cands) = candidate_docs {
                // Intersection
                *cands = cands.intersection(&docs).cloned().collect();
            } else {
                candidate_docs = Some(docs);
            }
        }
        
        let candidate_docs = match candidate_docs {
            Some(cands) if !cands.is_empty() => cands,
            _ => return results,
        };
        
        let key_lower = key.to_lowercase();
        let _total_docs = self.doc_ids.len().max(1);
        
        for doc_id in candidate_docs {
            // PHASE 1b: Read from text_store instead of doc_texts HashMap
            if let Some(doc_text) = self.get_text(&doc_id) {
                let doc_lower = doc_text.to_lowercase();
                
                // Find key position
                if let Some(key_pos) = doc_lower.find(&key_lower) {
                    let after_key = key_pos + key_lower.len();
                    let remainder = &doc_text[after_key..];
                    
                    // Skip separators (multi-layer filtering)
                    let mut value_start = remainder;
                    let mut skip_count = 0;
                    
                    // Layer 1: Skip whitespace and structural separators
                    while !value_start.is_empty() {
                        let first_char = value_start.chars().next().unwrap();
                        
                        // Connector tokens
                        if first_char.is_whitespace() || first_char == ':' || first_char == '=' {
                            value_start = &value_start[first_char.len_utf8()..];
                            skip_count += 1;
                            continue;
                        }
                        
                        // Check for connector words
                        let first_word: String = value_start.chars()
                            .take_while(|c| c.is_alphanumeric())
                            .collect();
                        
                        if CONNECTOR_TOKENS.contains(&first_word.to_lowercase().as_str()) && skip_count < 3 {
                            value_start = &value_start[first_word.len()..];
                            skip_count += 1;
                            continue;
                        }
                        
                        break;
                    }
                    
                    if !value_start.is_empty() {
                        // Extract value until delimiter
                        let value: String = value_start.chars()
                            .take_while(|c| !matches!(c, ',' | ';' | '\n' | '\r'))
                            .collect();
                        
                        let value = value.trim().to_string();
                        if !value.is_empty() {
                            results.push((value, doc_id.clone()));
                            
                            if top_k > 0 && results.len() >= top_k as usize {
                                return results;
                            }
                        }
                    }
                }
            }
        }
        
        results
    }
    
    /// Recall with CONTEXT snippet (like Google search results).
    /// Returns the text around the key, allowing user to extract value themselves.
    pub fn recall_context(&self, key: &str, context_chars: usize) -> Option<(String, String)> {
        let key_lower = key.to_lowercase();
        
        // Get candidate docs from inverted index first
        let key_words = self.simple_tokenize(key);
        let mut candidate_docs: HashSet<String> = HashSet::new();
        
        for word in &key_words {
            if let Some(docs) = self.word_inverted_index.get(word) {
                candidate_docs.extend(docs.iter().cloned());
            }
        }
        
        if candidate_docs.is_empty() {
            candidate_docs = self.doc_ids.iter().cloned().collect();
        }
        
        for doc_id in candidate_docs {
            // PHASE 1b: Read from text_store instead of doc_texts HashMap
            if let Some(doc_text) = self.get_text(&doc_id) {
                if let Some(idx) = doc_text.to_lowercase().find(&key_lower) {
                    let start = idx.saturating_sub(context_chars);
                    let end = (idx + key.len() + context_chars).min(doc_text.len());
                    let snippet = doc_text[start..end].to_string();
                    return Some((snippet, doc_id));
                }
            }
        }
        
        None
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // UTILITIES
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Clear all indexed documents.
    pub fn clear(&mut self) {
        // Clear optimized storage
        self.doc_id_to_idx.clear();
        self.text_store.clear();
        
        // Clear core storage
        self.doc_ids.clear();
        // doc_texts REMOVED
        self.doc_token_sets.clear();
        self.doc_token_counts.clear();
        self.doc_embeddings.clear();
        self.doc_passage_embeddings.clear();
        self.inverted_index.clear();
        self.position_tokens.clear();
        self.token_strings.clear();
        self.idf_cache.clear();
        self.word_vocabulary.clear();
        self.word_inverted_index.clear();
        self.word_idf.clear();
        self.word_doc_tf.clear();
        self.word_phonetic_index.clear();
        self.idf_dirty = true;
        self.doc_count = 0;
        // Quantized state (must reset when switching tasks e.g. needle -> WikimQA)
        self.matrix_quantized.clear();
        self.passage_counts.clear();
        self.passage_offsets.clear();
        self.doc_word_sets_fast.clear();
        self.doc_word_tf_fast.clear();
        self.doc_texts_fast.clear();
        self.word_idf_fast.clear();
        self.phonetic_index_fast.clear();
        self.word_inverted_fast.clear();
        self.word_vocab.clear();
        self.word_to_id.clear();
        self.quantized_mode = false;
        self.force_route = None;  // Reset so non-needle tasks use default routing
    }
    
    /// Get index statistics.
    pub fn stats(&self) -> HashMap<String, usize> {
        let total_tokens: usize = self.word_doc_tf.values().map(|m| m.len()).sum();
        // PHASE 1b: Use text_store.total_bytes() instead of doc_texts
        let total_chars: usize = self.text_store.total_bytes() as usize;
        let total_postings: usize = self.word_inverted_index.values().map(|s| s.len()).sum();
        
        let mut stats = HashMap::new();
        stats.insert("num_documents".to_string(), self.doc_ids.len());
        stats.insert("total_tokens".to_string(), total_tokens);
        stats.insert("total_chars".to_string(), total_chars);
        stats.insert("embeddings_cached".to_string(), self.doc_embeddings.len());
        stats.insert(
            "passage_embeddings_docs".to_string(),
            self.doc_passage_embeddings.len(),
        );
        let total_passages: usize = self.doc_passage_embeddings.values().map(|v| v.len()).sum();
        stats.insert("passage_embeddings_total".to_string(), total_passages);
        stats.insert("inverted_index_entries".to_string(), self.word_inverted_index.len());
        stats.insert("inverted_index_postings".to_string(), total_postings);
        stats.insert("art_size".to_string(), self.inverted_index.len());
        
        // Phase 1: New storage metrics
        stats.insert("text_store_docs".to_string(), self.text_store.doc_count());
        stats.insert("text_store_bytes".to_string(), self.text_store.total_bytes() as usize);
        stats.insert("doc_id_registry_size".to_string(), self.doc_id_to_idx.len());
        stats.insert("storage_persistent".to_string(), if self.text_store.is_persistent() { 1 } else { 0 });
        
        stats
    }
    
    /// Get document text by ID.
    /// PHASE 1b: Now reads from text_store instead of doc_texts HashMap
    pub fn get_document(&self, doc_id: &str) -> Option<String> {
        self.get_text(doc_id)
    }
    
    /// Get document text by ID from new TextStore (for A/B testing).
    /// Returns None if doc_id not found or storage read fails.
    pub fn get_document_from_store(&self, doc_id: &str) -> Option<String> {
        self.get_text(doc_id)
    }
    
    /// Verify text_store integrity - count docs with valid text.
    /// Returns (valid_docs, missing_docs) count.
    pub fn verify_storage_consistency(&self) -> (usize, usize) {
        let mut valid = 0;
        let mut missing = 0;
        
        for doc_id in &self.doc_ids {
            if self.get_text(doc_id).is_some() {
                valid += 1;
            } else {
                missing += 1;
            }
        }
        
        (valid, missing)
    }
    
    /// Check if document exists.
    pub fn has_document(&self, doc_id: &str) -> bool {
        // Use new storage - check if doc_id is in registry
        self.doc_id_to_idx.contains_key(doc_id)
    }
    
    /// Get document text by ID (internal helper for migration)
    /// Reads from NEW text_store instead of legacy doc_texts HashMap
    fn get_text(&self, doc_id: &str) -> Option<String> {
        let idx = self.doc_id_to_idx.get(doc_id)?;
        self.text_store.get_text(*idx).ok()
    }
    
    /// Number of indexed documents.
    pub fn num_documents(&self) -> usize {
        self.doc_ids.len()
    }

    /// Get all document IDs.
    pub fn get_doc_ids(&self) -> &Vec<String> {
        &self.doc_ids
    }

    /// Get the internal index for a doc_id (for entity matching in ScaEngine).
    pub fn get_doc_index(&self, doc_id: &str) -> Option<usize> {
        self.doc_id_to_idx.get(doc_id).map(|&idx| idx as usize)
    }

    /// Get document text by index (for entity matching in latent space search).
    pub fn get_doc_text_by_index(&self, idx: usize) -> Option<&str> {
        self.doc_texts_fast.get(idx).map(|s| s.as_str())
    }

    /// Per-doc normalized text (lowercase, word-level), owned. Falls back to reconstructing
    /// from the interned word set when doc_texts_fast is not cached (#4: it's no longer built
    /// at index time — the words live once in doc_word_sets_fast as u32 ids).
    pub fn doc_normalized_text(&self, idx: usize) -> Option<String> {
        if let Some(t) = self.doc_texts_fast.get(idx) {
            if !t.is_empty() { return Some(t.clone()); }
        }
        if idx < self.doc_word_sets_fast.len() {
            return Some(self.doc_words_joined(idx));
        }
        None
    }

    /// Per-structure approximate heap bytes of the lexical `_fast` index.
    /// Returns (doc_texts, doc_word_sets, doc_word_tf, word_inverted, phonetic, vocab).
    /// Counts String/key bytes + container slot overhead (rough but comparable + stable,
    /// so a memory-bound test can assert on it). This is the #4-OOM measurement surface.
    fn lexical_mem_parts(&self) -> (usize, usize, usize, usize, usize, usize) {
        fn s_bytes(s: &str) -> usize { s.len() + 24 } // String header ~24B + bytes
        let doc_texts: usize = self.doc_texts_fast.iter().map(|s| s_bytes(s)).sum();
        // doc_word_sets_fast now holds interned u32 ids (4 bytes each), not Strings.
        let doc_word_sets: usize = self.doc_word_sets_fast.iter()
            .map(|set| set.len() * 4 + 48).sum();
        // doc_word_tf_fast keys are interned u32 ids (4 bytes) + u32 count = 8 bytes/entry.
        let doc_word_tf: usize = self.doc_word_tf_fast.iter()
            .map(|m| m.len() * 8 + 48).sum();
        // word_inverted_fast keys are now interned u32 ids (4 bytes), not Strings.
        let word_inv: usize = self.word_inverted_fast.iter()
            .map(|(_id, set)| 4 + set.len() * 8 + 48).sum();
        // phonetic values are now interned u32 ids (4 bytes), not Strings.
        let phonetic: usize = self.phonetic_index_fast.iter()
            .map(|(k, set)| s_bytes(k) + set.len() * 4 + 48).sum();
        // the single interned vocab (word_vocab + word_to_id) — each unique word stored
        // ONCE in word_vocab + once as a word_to_id key (replaces the former ~9× String dup).
        let vocab: usize = self.word_vocab.iter().map(|w| s_bytes(w)).sum::<usize>()
            + self.word_to_id.iter().map(|(w, _)| s_bytes(w) + 4 + 8).sum::<usize>();
        (doc_texts, doc_word_sets, doc_word_tf, word_inv, phonetic, vocab)
    }

    /// Total approximate heap bytes held by the lexical `_fast` index (the #4 OOM driver).
    pub fn lexical_mem_bytes(&self) -> usize {
        let (a, b, c, d, e, f) = self.lexical_mem_parts();
        a + b + c + d + e + f
    }

    /// Heap bytes of the WORD-keyed lexical structures only — the part word-interning
    /// targets: doc_word_sets + doc_word_tf + word_inverted + phonetic + the single shared
    /// vocab. EXCLUDES doc_texts_fast (the raw per-doc text, which interning does not touch
    /// and which scales with content, not vocabulary). This is the honest #4 metric: the
    /// former String-keyed index stored each word ~9× here; interning collapses it to ~1×.
    pub fn lexical_word_index_bytes(&self) -> usize {
        let (_doc_texts, doc_word_sets, doc_word_tf, word_inv, phonetic, vocab) = self.lexical_mem_parts();
        doc_word_sets + doc_word_tf + word_inv + phonetic + vocab
    }

    /// Intern a word → compact u32 id, storing the canonical String exactly ONCE in
    /// `word_vocab`. Subsequent lookups of the same word return the existing id (no new
    /// allocation). This is the foundation for keying the lexical `_fast` structures on
    /// u32 instead of duplicating each word's String across all of them (#4 OOM fix).
    fn intern_word(&mut self, word: &str) -> u32 {
        if let Some(&id) = self.word_to_id.get(word) {
            return id;
        }
        let id = self.word_vocab.len() as u32;
        self.word_vocab.push(word.to_string());
        self.word_to_id.insert(word.to_string(), id);
        id
    }

    /// Resolve an interned id back to its word (None if out of range).
    #[allow(dead_code)]
    fn word_of(&self, id: u32) -> Option<&str> {
        self.word_vocab.get(id as usize).map(|s| s.as_str())
    }

    /// Convert a String-keyed term-frequency map into an interned-id-keyed one.
    fn intern_tf(&mut self, tf: ahash::AHashMap<String, u32>) -> ahash::AHashMap<u32, u32> {
        tf.into_iter().map(|(w, c)| (self.intern_word(&w), c)).collect()
    }

    /// Space-join a doc's interned word-id set back into a text string (phrase-match
    /// fallback used when doc_texts_fast is empty). Order is set-iteration order — same
    /// as the previous String-set behavior.
    fn doc_words_joined(&self, doc_idx: usize) -> String {
        match self.doc_word_sets_fast.get(doc_idx) {
            Some(set) => set.iter()
                .filter_map(|&id| self.word_of(id))
                .collect::<Vec<_>>()
                .join(" "),
            None => String::new(),
        }
    }

    /// Diagnostic: per-structure breakdown of `lexical_mem_bytes`. SAID_MEM_REPORT=1.
    /// Resident bytes of the quantized fingerprint matrix (grows per doc). #4 encode scale.
    pub fn matrix_quantized_bytes(&self) -> usize {
        self.matrix_quantized.len()
    }

    pub fn lexical_mem_report(&self) -> String {
        let (doc_texts, doc_word_sets, doc_word_tf, word_inv, phonetic, vocab) = self.lexical_mem_parts();
        let mb = |b: usize| (b as f64) / 1_048_576.0;
        let text_store = self.text_store.total_bytes() as usize; // the InMemoryTextStore raw-text copy
        format!(
            "lexical_mem (docs={}): doc_texts_fast={:.0}MB  doc_word_sets_fast={:.0}MB  doc_word_tf_fast={:.0}MB  word_inverted_fast={:.0}MB  phonetic_index_fast={:.0}MB  vocabulary_fast={:.0}MB  text_store={:.0}MB  | TOTAL={:.0}MB",
            self.doc_ids.len(),
            mb(doc_texts), mb(doc_word_sets), mb(doc_word_tf), mb(word_inv), mb(phonetic), mb(vocab), mb(text_store),
            mb(doc_texts + doc_word_sets + doc_word_tf + word_inv + phonetic + vocab + text_store),
        )
    }

    /// Clear only the word index structures (not quantized matrix or doc IDs).
    pub fn clear_word_index(&mut self) {
        self.word_inverted_fast.clear();
        self.word_vocab.clear();
        self.word_to_id.clear();
        self.doc_word_sets_fast.clear();
        self.doc_word_tf_fast.clear();
        self.doc_texts_fast.clear();
    }

    /// Add a single document's words to the word index.
    /// `words` should match the tokenization used during indexing (simple_tokenize).
    pub fn add_doc_to_word_index(&mut self, doc_idx: usize, words: &[String]) {
        use ahash::{AHashSet, AHashMap};

        let mut word_set = AHashSet::new();
        let mut word_tf: AHashMap<String, u32> = AHashMap::new();
        let mut doc_text = String::new();

        for w in words {
            if !doc_text.is_empty() { doc_text.push(' '); }
            doc_text.push_str(w);
            let wid = self.intern_word(w);
            word_set.insert(wid);
            *word_tf.entry(w.clone()).or_insert(0) += 1;
            self.word_inverted_fast
                .entry(wid)
                .or_insert_with(AHashSet::new)
                .insert(doc_idx);
        }

        // Ensure doc_word_sets_fast is large enough
        while self.doc_word_sets_fast.len() <= doc_idx {
            self.doc_word_sets_fast.push(AHashSet::new());
        }
        while self.doc_word_tf_fast.len() <= doc_idx {
            self.doc_word_tf_fast.push(AHashMap::new());
        }
        while self.doc_texts_fast.len() <= doc_idx {
            self.doc_texts_fast.push(String::new());
        }

        self.doc_word_sets_fast[doc_idx] = word_set;
        self.doc_word_tf_fast[doc_idx] = self.intern_tf(word_tf);
        self.doc_texts_fast[doc_idx] = doc_text;
    }

    /// Rebuild word inverted index + doc word sets from full original texts.
    /// Call after deserialization when the stored tokenized text differs from originals.
    /// Rebuild word index from raw texts — MUST match add_docs_quantized exactly.
    ///
    /// Input: raw original texts (same as passed to index_batch).
    /// Internally applies simple_tokenize (lowercase + len>=3) THEN punct strip,
    /// matching the exact pipeline in add_docs_quantized.
    /// Rebuild word index from raw texts — MUST match add_docs_quantized exactly.
    /// Two-phase: (1) parallel per-doc tokenization, (2) sequential merge of shared structures.
    pub fn rebuild_word_index_from_texts(&mut self, texts: &[String]) {
        use ahash::{AHashSet, AHashMap};
        use rayon::prelude::*;

        self.word_inverted_fast.clear();
        self.word_vocab.clear();
        self.word_to_id.clear();
        self.doc_word_sets_fast.clear();
        self.doc_word_tf_fast.clear();
        self.doc_texts_fast.clear();
        self.phonetic_index_fast.clear();

        // Phase 1: Parallel per-doc tokenization (no shared state)
        struct DocResult {
            word_set: AHashSet<String>,
            word_tf: AHashMap<String, u32>,
            doc_text: String,
            unique_words: Vec<String>, // for inverted index merge
        }

        let results: Vec<DocResult> = texts.par_iter().map(|text| {
            let words: Vec<String> = text.split_whitespace()
                .map(|w| w.to_lowercase())
                .filter(|w| w.len() >= 3)
                .collect();

            let mut word_set = AHashSet::new();
            let mut word_tf: AHashMap<String, u32> = AHashMap::new();
            let mut doc_text = String::with_capacity(text.len());

            for w in &words {
                let w_normalized: String = w
                    .trim_end_matches(|c: char| c.is_ascii_punctuation())
                    .to_string();

                if !doc_text.is_empty() { doc_text.push(' '); }
                doc_text.push_str(&w_normalized);

                if w_normalized.len() < 3 { continue; }
                word_set.insert(w_normalized.clone());
                *word_tf.entry(w_normalized.clone()).or_insert(0) += 1;
            }

            let unique_words: Vec<String> = word_set.iter().cloned().collect();
            DocResult { word_set, word_tf, doc_text, unique_words }
        }).collect();

        // Phase 2: Sequential merge (builds shared inverted index + phonetic + vocabulary)
        for (doc_idx, result) in results.into_iter().enumerate() {
            for word in &result.unique_words {
                let wid = self.intern_word(word);
                self.word_inverted_fast
                    .entry(wid)
                    .or_insert_with(AHashSet::new)
                    .insert(doc_idx);


                let sx = self.get_soundex(word);
                self.phonetic_index_fast
                    .entry(sx)
                    .or_insert_with(AHashSet::new)
                    .insert(wid);
            }

            // Convert this doc's word-set (Strings) to interned ids (all already interned
            // in the loop above, so this just resolves them).
            let id_set: ahash::AHashSet<u32> = result.word_set.iter()
                .map(|w| self.intern_word(w)).collect();
            self.doc_word_sets_fast.push(id_set);
            let tf_ids = self.intern_tf(result.word_tf); self.doc_word_tf_fast.push(tf_ids);
            self.doc_texts_fast.push(String::new()); // #4: not cached
        }
    }

    /// Public ART prefilter for use by engine.rs search_mteb_focused.
    pub fn art_prefilter_public(&self, q_expanded: &ahash::AHashSet<String>, query_text: &str) -> Option<Vec<usize>> {
        self.art_prefilter(q_expanded, query_text)
    }

    /// True if doc `doc_idx` contains `word` (resolves the word to its interned id).
    /// Replaces the former `get_doc_word_set(...).contains(word)` pattern now that the
    /// per-doc sets key on u32 ids (#4 interning).
    pub fn doc_has_word(&self, doc_idx: usize, word: &str) -> bool {
        match (self.word_to_id.get(word), self.doc_word_sets_fast.get(doc_idx)) {
            (Some(id), Some(set)) => set.contains(id),
            _ => false,
        }
    }

    /// Get doc word-id set by index (internal lexical scoring).
    pub fn get_doc_word_set(&self, doc_idx: usize) -> Option<&ahash::AHashSet<u32>> {
        self.doc_word_sets_fast.get(doc_idx)
    }

    /// Get per-doc word→TF map (for TF-IDF scoring in FullHybrid route).
    /// Matches SAID-LAM-private's `doc_word_tf[doc_idx]` access pattern.
    pub fn get_doc_word_tf(&self, doc_idx: usize) -> Option<&ahash::AHashMap<u32, u32>> {
        self.doc_word_tf_fast.get(doc_idx)
    }

    /// True when there is no resident word index to serialize (nothing indexed yet).
    pub fn word_index_is_empty(&self) -> bool {
        self.word_inverted_fast.is_empty() && self.doc_word_sets_fast.is_empty()
    }

    /// Attach decompressed WIDX bytes loaded from a `.said` file's WIDX section (open() calls this).
    pub fn set_widx_bytes(&mut self, bytes: Option<Vec<u8>>) {
        self.widx_bytes = bytes;
    }

    /// True if a disk-backed word index is attached (queries can read postings in place).
    pub fn has_widx(&self) -> bool {
        self.widx_bytes.is_some()
    }

    /// Build a borrowing reader over the attached WIDX bytes (cheap — just an offset scan). None if
    /// no WIDX is attached or the bytes are malformed.
    pub fn widx_reader(&self) -> Option<crate::word_index::WidxReader<'_>> {
        let bytes = self.widx_bytes.as_deref()?;
        crate::word_index::WidxReader::new(bytes).ok()
    }

    /// Build a serialization-ready `WordIndex` (WIDX) from the resident BM25 structures. Every list
    /// is SORTED so the byte format is deterministic + delta-encodable and the mmap reader can
    /// binary-search. This is the write side of the disk-backed word index (the 580MB fix): save()
    /// calls this, serializes it into the WIDX section, and the resident HashMaps can then be dropped.
    pub fn to_word_index(&self) -> crate::word_index::WordIndex {
        let doc_word_sets: Vec<Vec<u32>> = self.doc_word_sets_fast.iter()
            .map(|set| { let mut v: Vec<u32> = set.iter().copied().collect(); v.sort_unstable(); v })
            .collect();
        let doc_word_tf: Vec<Vec<(u32, u32)>> = self.doc_word_tf_fast.iter()
            .map(|m| { let mut v: Vec<(u32, u32)> = m.iter().map(|(&k, &c)| (k, c)).collect(); v.sort_unstable_by_key(|&(k, _)| k); v })
            .collect();
        let mut word_inverted: Vec<(u32, Vec<u32>)> = self.word_inverted_fast.iter()
            .map(|(&wid, docs)| { let mut d: Vec<u32> = docs.iter().map(|&x| x as u32).collect(); d.sort_unstable(); (wid, d) })
            .collect();
        word_inverted.sort_unstable_by_key(|&(wid, _)| wid);
        let mut phonetic: Vec<(String, Vec<u32>)> = self.phonetic_index_fast.iter()
            .map(|(sx, wids)| { let mut w: Vec<u32> = wids.iter().copied().collect(); w.sort_unstable(); (sx.clone(), w) })
            .collect();
        phonetic.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        crate::word_index::WordIndex {
            vocab: self.word_vocab.clone(),
            doc_word_sets,
            doc_word_tf,
            word_inverted,
            phonetic,
        }
    }

    /// Get IDF for a word (for lexical scoring in engine.rs).
    pub fn get_word_idf(&self, word: &str) -> f32 {
        *self.word_idf_fast.get(word).unwrap_or(&1.0)
    }

    /// Get doc_id by index.
    pub fn doc_id_at(&self, idx: usize) -> Option<&str> {
        self.doc_ids.get(idx).map(|s| s.as_str())
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // THE SAID STANDARD: Production API
    // ═══════════════════════════════════════════════════════════════════════════
    // Simple as sentence-transformers. Two methods:
    //   encode() → Lock document
    //   recall() → Auto-routing search (handles everything)
    //
    // Internal methods (search_kv, search_all_instances) available for testing.
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// SAID Standard: Encode document into crystalline state.
    /// Simple API aligned with sentence-transformers.
    pub fn encode(&mut self, doc_id: &str, text: &str, chunk_size: usize) -> HashMap<String, usize> {
        self.stream_index(doc_id, text, chunk_size)
    }
    
    /// SAID Standard: Deterministic Recall (auto-routes everything).
    /// Simple API - one method handles all query types.
    pub fn recall(
        &mut self,
        query: &str,
        top_k: usize,
        query_embedding: Option<&[f32]>,
        alpha_override: Option<f32>,
    ) -> Vec<(String, f32)> {
        self.search(query, top_k, query_embedding, alpha_override)
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // QUANTIZED SEARCH (sca_dropin alignment for 94.1060 parity)
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Set corpus mean for quantization (call before add_docs_quantized)
    pub fn set_corpus_mean(&mut self, mean: Vec<f32>) {
        self.dim = mean.len();
        self.quantized_dim = (self.dim + 7) / 8;
        self.bytes_per_passage = if self.holographic_16view {
            self.quantized_dim * 16
        } else {
            self.quantized_dim
        };
        self.corpus_mean = mean;
    }

    /// Set per-dimension standard deviation for whitened binarization.
    pub fn set_corpus_std(&mut self, std: Vec<f32>) {
        self.corpus_std = std;
    }

    /// Get corpus mean (for brain cross-timescale drift).
    pub fn get_corpus_mean(&self) -> &[f32] {
        &self.corpus_mean
    }

    /// Get corpus std (for brain cross-timescale drift).
    pub fn get_corpus_std(&self) -> &[f32] {
        &self.corpus_std
    }
    
    /// Enable holographic 16-view quantization
    pub fn set_holographic_16view(&mut self, enabled: bool, scale: Option<f32>) {
        self.holographic_16view = enabled;
        if let Some(s) = scale {
            self.holographic_scale = s;
        }
        self.bytes_per_passage = if enabled {
            self.quantized_dim * 16
        } else {
            self.quantized_dim
        };
    }
    
    /// Load IDF scores (sca_dropin style)
    pub fn load_idf_fast(&mut self, words: Vec<String>, scores: Vec<f32>) {
        for (w, s) in words.iter().zip(scores.iter()) {
            let w_lower = w.to_lowercase();
            self.word_idf_fast.insert(w_lower, *s);
        }
    }

    /// Force routing override (e.g. "FullHybrid" for LEMBNeedleRetrieval).
    pub fn set_force_route(&mut self, route_str: &str) {
        self.force_route = match route_str.trim() {
            "PureSemantic" => Some(QueryRoute::PureSemantic),
            "FullHybrid" => Some(QueryRoute::FullHybrid),
            "PureLexical" => Some(QueryRoute::PureLexical),
            _ => None,
        };
    }

    /// Extended qrels: check if both docs contain valid answer (needle overlap).
    /// MATCHES sca_dropin/lam_scientific_proof_suite _check_both_docs_valid exactly.
    pub fn check_both_docs_valid_quantized(&self, query: &str, doc1_id: &str, doc2_id: &str) -> bool {
        let stopwords: AHashSet<&str> = [
            "what", "when", "where", "which", "who", "why", "how", "the", "they",
            "them", "their", "known", "that", "this", "with", "from", "have", "been",
            "were", "being", "for", "was", "and", "are", "is", "his", "her", "she", "he"
        ].iter().cloned().collect();
        let query_lower = query.to_lowercase();
        let keywords: Vec<&str> = query_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 4 && !stopwords.contains(w))
            .collect();
        if keywords.is_empty() { return false; }
        let doc1_idx = self.doc_id_to_idx.get(doc1_id).map(|&v| v as usize);
        let doc2_idx = self.doc_id_to_idx.get(doc2_id).map(|&v| v as usize);
        let (doc1_text, doc2_text) = match (doc1_idx, doc2_idx) {
            (Some(i1), Some(i2)) => {
                if !self.doc_texts_fast.is_empty() && i1 < self.doc_texts_fast.len() && i2 < self.doc_texts_fast.len() {
                    (self.doc_texts_fast[i1].clone(), self.doc_texts_fast[i2].clone())
                } else if i1 < self.doc_word_sets_fast.len() && i2 < self.doc_word_sets_fast.len() {
                    (self.doc_words_joined(i1), self.doc_words_joined(i2))
                } else {
                    return false;
                }
            }
            _ => return false,
        };
        let doc1_hits = keywords.iter().filter(|kw| doc1_text.contains(*kw)).count();
        let doc2_hits = keywords.iter().filter(|kw| doc2_text.contains(*kw)).count();
        let threshold = (keywords.len() as f32 * 0.5) as usize;
        doc1_hits >= threshold && doc2_hits >= threshold
    }

    /// Compute keyword overlap hits for a single document to enable needle re-ranking.
    pub fn compute_keyword_hits_quantized(&self, query: &str, doc_id: &str) -> usize {
        let stopwords: AHashSet<&str> = [
            "what", "when", "where", "which", "who", "why", "how", "the", "they",
            "them", "their", "known", "that", "this", "with", "from", "have", "been",
            "were", "being", "for", "was", "and", "are", "is", "his", "her", "she", "he"
        ].iter().cloned().collect();
        let query_lower = query.to_lowercase();
        let keywords: Vec<&str> = query_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| (w.len() >= 2 || (w.len() >= 1 && w.chars().any(|c| c.is_ascii_digit()))) && !stopwords.contains(w))
            .collect();
        if keywords.is_empty() { return 0; }
        
        let doc_idx = self.doc_id_to_idx.get(doc_id).map(|&v| v as usize);
        let doc_text = match doc_idx {
            Some(i) => {
                if !self.doc_texts_fast.is_empty() && i < self.doc_texts_fast.len() {
                    self.doc_texts_fast[i].clone() // already lowercase
                } else if i < self.doc_word_sets_fast.len() {
                    self.doc_words_joined(i)
                } else {
                    return 0;
                }
            }
            _ => return 0,
        };
        keywords.iter().filter(|kw| doc_text.contains(*kw)).count()
    }

    /// Retrieve and rank ALL documents in the corpus purely by keyword intersection hits for Needle tasks.
    pub fn get_highest_keyword_overlap_docs(&self, query: &str) -> Vec<(String, usize)> {
        let mut results = Vec::new();
        // pre-extract to avoid re-extracting per doc
        let stopwords: AHashSet<&str> = [
            "what", "when", "where", "which", "who", "why", "how", "the", "they",
            "them", "their", "known", "that", "this", "with", "from", "have", "been",
            "were", "being", "for", "was", "and", "are", "is", "his", "her", "she", "he"
        ].iter().cloned().collect();
        let query_lower = query.to_lowercase();
        let keywords: Vec<&str> = query_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| (w.len() >= 2 || (w.len() >= 1 && w.chars().any(|c| c.is_ascii_digit()))) && !stopwords.contains(w))
            .collect();
            
        if keywords.is_empty() { return results; }

        for (i, doc_id) in self.doc_ids.iter().enumerate() {
            let doc_text = if !self.doc_texts_fast.is_empty() && i < self.doc_texts_fast.len() {
                self.doc_texts_fast[i].to_lowercase()
            } else if i < self.doc_word_sets_fast.len() {
                self.doc_words_joined(i)
            } else {
                continue;
            };
            
            let hits = keywords.iter().filter(|kw| doc_text.contains(**kw)).count();
            if hits > 0 {
                results.push((doc_id.clone(), hits));
            }
        }
        // sort descending
        results.sort_by(|a, b| b.1.cmp(&a.1));
        results.truncate(100);
        results
    }
    
    /// Evaluate one query with extended qrels. Returns (is_correct, used_extended).
    pub fn evaluate_query_quantized(
        &self,
        query_emb: &[f32],
        query_text: &str,
        expected_doc_ids: &[String],
        top_k: usize,
    ) -> (bool, bool) {
        let results = self.search_unified_quantized(query_emb, query_text, top_k);
        if results.is_empty() { return (false, false); }
        let top_doc_id = &results[0].0;
        if expected_doc_ids.iter().any(|id| id == top_doc_id) {
            return (true, false);
        }
        for expected_id in expected_doc_ids {
            if self.check_both_docs_valid_quantized(query_text, top_doc_id, expected_id) {
                return (true, true);
            }
        }
        (false, false)
    }

    /// Batch evaluate with extended qrels. Returns (correct_count, extended_count, total).
    pub fn evaluate_batch_quantized(
        &self,
        query_embeddings: &[Vec<f32>],
        query_texts: &[String],
        expected_doc_ids_list: &[Vec<String>],
        top_k: usize,
    ) -> (usize, usize, usize) {
        let mut correct = 0usize;
        let mut extended = 0usize;
        let total = query_texts.len();
        for i in 0..total {
            let q_emb = query_embeddings.get(i).map(|v| v.as_slice()).unwrap_or(&[]);
            let q_text = query_texts.get(i).map(|s| s.as_str()).unwrap_or("");
            let expected = expected_doc_ids_list.get(i).map(|v| v.as_slice()).unwrap_or(&[]);
            let (is_correct, used_ext) = self.evaluate_query_quantized(q_emb, q_text, expected, top_k);
            if is_correct {
                correct += 1;
                if used_ext { extended += 1; }
            }
        }
        (correct, extended, total)
    }

    /// MAD-based dynamic scale for holographic 16-view
    fn compute_dynamic_scale(&self, embeddings_flat: &[f32]) -> f32 {
        if embeddings_flat.is_empty() || self.corpus_mean.is_empty() || self.dim == 0 {
            return 0.2;
        }
        let n = embeddings_flat.len() / self.dim;
        let mut flat: Vec<f32> = Vec::with_capacity(n * self.dim);
        for chunk in embeddings_flat.chunks(self.dim) {
            if chunk.len() != self.dim {
                continue;
            }
            for (d, &c) in chunk.iter().enumerate() {
                let m = self.corpus_mean.get(d).copied().unwrap_or(0.0);
                flat.push((c - m).abs());
            }
        }
        if flat.is_empty() {
            return 0.2;
        }
        flat.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let s = flat[flat.len() / 2];
        let scale = 2.0 * s;
        scale.clamp(0.05, 0.5)
    }
    
    /// Add documents with quantized embeddings (sca_dropin style bulk indexing)
    /// 
    /// Args:
    ///   ids: Document IDs
    ///   embeddings_flat: Flattened embeddings (all passages for all docs)
    ///   passage_counts: Number of passages per document
    ///   gammas: Per-document gamma values (unused but kept for API compat)
    ///   doc_words: Words for each document (for lexical indexing)
    pub fn add_docs_quantized(
        &mut self,
        ids: Vec<String>,
        embeddings_flat: Vec<f32>,
        passage_counts: Vec<usize>,
        _gammas: Vec<f32>,
        // #4 memory: take the per-doc TEXT (borrowed) and tokenize one doc at a time inside
        // the word-index loop below, instead of receiving a pre-built Vec<Vec<String>> of
        // EVERY word in EVERY doc (~237MB transient on a text-heavy chunk). The caller already
        // holds these texts; we borrow them and never materialize the full word list.
        doc_texts: &[String],
    ) {
        self.quantized_mode = true;
        let start_idx = self.doc_ids.len();
        
        // Add doc_ids + doc_id_to_idx mapping
        for (i, id) in ids.iter().enumerate() {
            let idx = start_idx + i;
            self.doc_ids.push(id.clone());
            self.doc_id_to_idx.insert(id.clone(), idx as u64);
        }
        
        // Track passage offsets for MaxSim lookup
        let stride = self.bytes_per_passage;
        let mut current_offset = self.matrix_quantized.len() / stride.max(1);
        for &passage_count in &passage_counts {
            self.passage_counts.push(passage_count);
            self.passage_offsets.push(current_offset);
            current_offset += passage_count;
        }
        
        // Word-Level Indexing (sca_dropin style). The pure per-doc work — tokenize, lowercase,
        // punctuation-strip, soundex — is independent across docs and was ~60% of phase-2 time
        // when serial (#4 throughput). We do it in PARALLEL (par_iter over docs, no shared
        // state), producing for each doc the ordered list of (normalized_word, soundex) plus a
        // tf map; then we MERGE into the shared interned structures SEQUENTIALLY in doc order,
        // so the vocab ids and every index are bit-identical to the old serial build. The merge
        // is cheap (HashMap inserts); the CPU-heavy tokenize/lowercase/soundex is parallel.
        struct DocWords {
            /// (normalized_word, soundex) in first-seen order; only words with len>=3.
            indexed: Vec<(String, String)>,
            tf: AHashMap<String, u32>,
        }
        // CRITICAL: the original add_docs_quantized received `doc_words` that were produced by
        // ScaEngine::simple_tokenize (split_whitespace + lowercase + len>=3, NO regex, NO
        // Porter2 stem). When #4 made this fn tokenize internally, an earlier revision wrongly
        // used the STEMMED crystalline::simple_tokenize here — which silently rebuilt the whole
        // BM25 word index from different tokens and dropped recall@10 0.95→0.90. We MUST match
        // the engine's whitespace tokenizer exactly to keep the lexical index identical.
        // 580MB CEILING (#4): the old code did one `doc_texts.par_iter().collect::<Vec<DocWords>>()`
        // over the WHOLE corpus — every doc's word list + soundex + tf map held in RAM at once. On a
        // 37k-frame SQL corpus that transient was multiple GB (a second whole-corpus materialization
        // beyond the encode phase). We process doc_texts in BOUNDED WINDOWS: par-prepare a window,
        // merge it sequentially, drop it, next window. Peak transient = one window, not the corpus.
        // Windows are contiguous in doc order and merged in order, so vocab-id assignment + every
        // index is BIT-IDENTICAL to the old single-collect build.
        let word_budget: usize = std::env::var("SAID_INDEX_BUDGET")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|&b: &usize| b > 0)
            .unwrap_or(580 * 1024 * 1024);
        // Rough transient cost per doc: avg text bytes × a factor for the (word String + soundex
        // String + tf entry) expansion. Sample to keep huge-SQL-file corpora windowing tight.
        let sample_n = doc_texts.len().min(64);
        let avg_text: usize = if sample_n == 0 { 1 } else {
            (doc_texts[..sample_n].iter().map(|t| t.len()).sum::<usize>() / sample_n).max(1)
        };
        let per_doc_cost = avg_text * 6; // ~6× text size for the DocWords expansion
        let word_window = (word_budget / per_doc_cost.max(1)).clamp(1, doc_texts.len().max(1));

        let mut merged = 0usize;
        for chunk in doc_texts.chunks(word_window) {
            let prepared: Vec<DocWords> = chunk
                .par_iter()
                .map(|text| {
                    let words: Vec<String> = text
                        .split_whitespace()
                        .map(|w| w.to_lowercase())
                        .filter(|w| w.len() >= 3)
                        .collect();
                    let mut indexed: Vec<(String, String)> = Vec::with_capacity(words.len());
                    let mut tf: AHashMap<String, u32> = AHashMap::new();
                    for w in &words {
                        let w_lower = w.to_lowercase();
                        let w_normalized: String = w_lower
                            .trim_end_matches(|c: char| c.is_ascii_punctuation())
                            .to_string();
                        if w_normalized.len() < 3 {
                            continue;
                        }
                        *tf.entry(w_normalized.clone()).or_insert(0) += 1;
                        let sx = Self::get_soundex_static(&w_normalized);
                        indexed.push((w_normalized, sx));
                    }
                    DocWords { indexed, tf }
                })
                .collect();

            // Sequential merge — identical vocab-id assignment + index population as the old loop.
            for dw in prepared.into_iter() {
                let doc_idx = start_idx + merged;
                let mut word_set = AHashSet::new();
                for (w_normalized, sx) in &dw.indexed {
                    let wid = self.intern_word(w_normalized);
                    word_set.insert(wid);
                    self.word_inverted_fast
                        .entry(wid)
                        .or_insert_with(AHashSet::new)
                        .insert(doc_idx);
                    self.phonetic_index_fast
                        .entry(sx.clone())
                        .or_insert_with(AHashSet::new)
                        .insert(wid);
                }
                self.doc_word_sets_fast.push(word_set);
                let tf_ids = self.intern_tf(dw.tf); self.doc_word_tf_fast.push(tf_ids);
                self.doc_texts_fast.push(String::new()); // #4: not cached; readers fall back to doc_words_joined
                merged += 1;
            }
        }
        
        // Compute dynamic scale if holographic
        if self.holographic_16view {
            self.holographic_scale = self.compute_dynamic_scale(&embeddings_flat);
        }

        // Quantize all embeddings (16-view holographic or standard 1-bit)
        let chunks: Vec<&[f32]> = embeddings_flat.chunks(self.dim).collect();
        let h_scale = self.holographic_scale;
        let h16 = self.holographic_16view;
        let q_dim = self.quantized_dim;
        let dim = self.dim;
        let corpus_mean = &self.corpus_mean;
        let corpus_std = &self.corpus_std;

        let processed: Vec<Vec<u8>> = chunks.par_iter().map(|emb| {
            if h16 {
                // 16-VIEW HOLOGRAPHIC QUANTIZATION
                let mut out = Vec::with_capacity(q_dim * 16);
                for i in 0..16 {
                    let off = (i as f32 / 15.0 - 0.5) * h_scale;
                    let mut packed = vec![0u8; q_dim];
                    
                    for d in 0..dim {
                        let mean_val = corpus_mean.get(d).copied().unwrap_or(0.0);
                        let val = emb.get(d).copied().unwrap_or(0.0) - mean_val + off;
                        if val > 0.0 {
                            let byte_idx = d / 8;
                            let bit_idx = d % 8;
                            if byte_idx < packed.len() {
                                packed[byte_idx] |= 1 << bit_idx;
                            }
                        }
                    }
                    out.extend(packed);
                }
                out
            } else {
                // WHITENED 1-BIT QUANTIZATION: sign((emb - mean) / std)
                // Equalizes bit importance — each dimension contributes equally
                let mut packed = vec![0u8; q_dim];
                for d in 0..dim {
                    let mean_val = corpus_mean.get(d).copied().unwrap_or(0.0);
                    let std_val = corpus_std.get(d).copied().unwrap_or(1.0);
                    let val = (emb.get(d).copied().unwrap_or(0.0) - mean_val) / std_val;
                    if val > 0.0 {
                        let byte_idx = d / 8;
                        let bit_idx = d % 8;
                        if byte_idx < packed.len() {
                            packed[byte_idx] |= 1 << bit_idx;
                        }
                    }
                }
                packed
            }
        }).collect();
        
        for bits in processed {
            self.matrix_quantized.extend(bits);
        }

        // Set rerank_depth to total docs
        self.rerank_depth = self.doc_ids.len();
    }

    /// Build word-level structures from raw texts (no embeddings needed).
    /// Used by index_stream_only() for privacy/streaming mode.
    /// Replicates the word indexing from add_docs_quantized + IDF from index_mteb_quantized
    /// but skips passage creation, model inference, and quantization.
    pub fn build_word_structures_from_texts(&mut self, ids: &[String], texts: &[String]) {
        // A. Build doc_freq for IDF computation (matches engine.rs index_mteb_quantized)
        let mut doc_freq: AHashMap<String, usize> = AHashMap::new();
        let mut doc_word_lists: Vec<Vec<String>> = Vec::with_capacity(texts.len());

        for text in texts {
            let words: Vec<String> = text.split_whitespace()
                .map(|w| w.to_lowercase())
                .filter(|w| w.len() >= 3)
                .collect();

            let mut unique_words = words.clone();
            unique_words.sort();
            unique_words.dedup();

            for w in unique_words {
                *doc_freq.entry(w).or_insert(0) += 1;
            }
            doc_word_lists.push(words);
        }

        // B. Compute IDF: ln((N+1)/(freq+1)) + 1.0
        let n = texts.len() as f64;
        let mut idf_keys = Vec::new();
        let mut idf_values = Vec::new();

        for (w, freq) in &doc_freq {
            let score = ((n + 1.0) / (*freq as f64 + 1.0)).ln() + 1.0;
            let score_f32 = score as f32;
            idf_keys.push(w.clone());
            idf_values.push(score_f32);
        }
        self.load_idf_fast(idf_keys, idf_values);

        // C. Word-level indexing (replicates add_docs_quantized lines 2402-2447)
        // doc_ids are already set by stream_index, so find start_idx
        let start_idx = self.doc_ids.len().saturating_sub(ids.len());

        for (i, words) in doc_word_lists.iter().enumerate() {
            let doc_idx = start_idx + i;
            let mut word_set = AHashSet::new();
            let mut word_tf: AHashMap<String, u32> = AHashMap::new();
            let mut doc_text = String::new();

            for w in words {
                let w_normalized: String = w
                    .trim_end_matches(|c: char| c.is_ascii_punctuation())
                    .to_string();

                if !doc_text.is_empty() {
                    doc_text.push(' ');
                }
                doc_text.push_str(&w_normalized);

                if w_normalized.len() < 3 {
                    continue;
                }

                let wid = self.intern_word(&w_normalized);
                word_set.insert(wid);
                *word_tf.entry(w_normalized.clone()).or_insert(0) += 1;

                self.word_inverted_fast
                    .entry(wid)
                    .or_insert_with(AHashSet::new)
                    .insert(doc_idx);

                let sx = self.get_soundex(&w_normalized);
                self.phonetic_index_fast
                    .entry(sx)
                    .or_insert_with(AHashSet::new)
                    .insert(wid);

            }

            self.doc_word_sets_fast.push(word_set);
            let tf_ids = self.intern_tf(word_tf); self.doc_word_tf_fast.push(tf_ids);
            self.doc_texts_fast.push(String::new()); // #4: not cached; readers fall back to doc_words_joined
        }

        self.rerank_depth = self.doc_ids.len();
    }

    /// QJL asymmetric similarity: binary doc fingerprint × float query.
    /// Returns similarity score (higher = more similar).
    /// doc_bits: packed sign bits from quantization
    /// query_centered: query_emb - corpus_mean (NOT quantized)
    #[inline]
    fn asymmetric_similarity(doc_bits: &[u8], query_centered: &[f32], dim: usize) -> f32 {
        let mut score = 0.0f32;
        for d in 0..dim {
            let byte_idx = d / 8;
            let bit_idx = d % 8;
            let bit_set = if byte_idx < doc_bits.len() {
                (doc_bits[byte_idx] >> bit_idx) & 1 == 1
            } else {
                false
            };
            let q_val = query_centered.get(d).copied().unwrap_or(0.0);
            // bit=1 means doc was positive → add; bit=0 means negative → subtract
            if bit_set {
                score += q_val;
            } else {
                score -= q_val;
            }
        }
        score
    }

    /// QJL asymmetric distance: converts similarity to distance for compatibility.
    /// Returns a distance (lower = more similar) scaled to [0, max_hamming] range.
    ///
    /// Key insight: for unit-normalized query_centered, dot(sign_doc, q) correlates
    /// with cosine similarity. We normalize by L2 norm (not L1) to get [-1, +1]
    /// similarity, then map to Hamming-compatible distance [0, max_hamming].
    fn asymmetric_distance(doc_bits: &[u8], query_centered: &[f32], dim: usize, max_hamming: f64) -> f64 {
        let raw_sim = Self::asymmetric_similarity(doc_bits, query_centered, dim) as f64;

        // Normalize by L2 norm of centered query to get cosine-like similarity in [-1, 1]
        let l2_norm = (query_centered.iter().map(|v| (v * v) as f64).sum::<f64>()).sqrt();
        if l2_norm < 1e-12 { return max_hamming / 2.0; }

        // Also need to account for the "magnitude" of the binary vector:
        // sign bits have effective L2 norm = sqrt(dim)
        let binary_l2 = (dim as f64).sqrt();

        // Cosine-like similarity: dot(sign, q) / (||sign||₂ * ||q||₂)
        let cos_sim = raw_sim / (binary_l2 * l2_norm);

        // Map from [-1, +1] similarity to [max_hamming, 0] distance
        // cos_sim = 1.0 → distance = 0 (identical)
        // cos_sim = -1.0 → distance = max_hamming (opposite)
        // cos_sim = 0.0 → distance = max_hamming/2 (orthogonal)
        let normalized_sim = (cos_sim + 1.0) / 2.0; // [0, 1]
        (1.0 - normalized_sim) * max_hamming
    }

    /// Center query for asymmetric search (subtract corpus mean, keep as floats)
    pub(crate) fn center_query(&self, query_emb: &[f32]) -> Vec<f32> {
        (0..self.dim)
            .map(|d| {
                let q = query_emb.get(d).copied().unwrap_or(0.0);
                let m = self.corpus_mean.get(d).copied().unwrap_or(0.0);
                q - m
            })
            .collect()
    }

    /// Quantize a query embedding
    pub(crate) fn quantize_query(&self, query_emb: &[f32]) -> Vec<u8> {
        if self.holographic_16view {
            let h_scale = self.holographic_scale;
            let mut q_bits = Vec::with_capacity(self.quantized_dim * 16);
            
            for i in 0..16 {
                let off = (i as f32 / 15.0 - 0.5) * h_scale;
                let mut packed = vec![0u8; self.quantized_dim];
                
                for d in 0..self.dim {
                    let mean_val = self.corpus_mean.get(d).copied().unwrap_or(0.0);
                    let val = query_emb.get(d).copied().unwrap_or(0.0) - mean_val + off;
                    if val > 0.0 {
                        let byte_idx = d / 8;
                        let bit_idx = d % 8;
                        if byte_idx < packed.len() {
                            packed[byte_idx] |= 1 << bit_idx;
                        }
                    }
                }
                q_bits.extend(packed);
            }
            q_bits
        } else {
            // Whitened 1-bit: sign((emb - mean) / std)
            let mut packed = vec![0u8; self.quantized_dim];
            for d in 0..self.dim {
                let mean_val = self.corpus_mean.get(d).copied().unwrap_or(0.0);
                let std_val = self.corpus_std.get(d).copied().unwrap_or(1.0);
                let val = (query_emb.get(d).copied().unwrap_or(0.0) - mean_val) / std_val;
                if val > 0.0 {
                    let byte_idx = d / 8;
                    let bit_idx = d % 8;
                    if byte_idx < packed.len() {
                        packed[byte_idx] |= 1 << bit_idx;
                    }
                }
            }
            packed
        }
    }
    
    /// Hamming candidate retrieval (pure semantic path)
    fn hamming_candidate_retrieval(&self, q_bits: &[u8], limit: usize) -> Vec<(usize, f64)> {
        let stride = self.bytes_per_passage;
        let max_hamming = (self.quantized_dim * 8) as f64;
        let holo16 = self.holographic_16view && q_bits.len() >= 16 * self.quantized_dim;
        let num_docs = self.doc_ids.len();
        
        let mut candidates: Vec<(usize, f64)> = if !self.passage_counts.is_empty() &&
            self.passage_counts.iter().any(|&c| c > 1) {
            // PASSAGE MODE: Find min Hamming across all passages per doc
            (0..num_docs).into_par_iter().map(|doc_idx| {
                let passage_count = self.passage_counts.get(doc_idx).copied().unwrap_or(1);
                let passage_offset = self.passage_offsets.get(doc_idx).copied().unwrap_or(doc_idx);
                let mut min_dist = f64::MAX;
                
                for p_idx in 0..passage_count {
                    let start = (passage_offset + p_idx) * stride;
                    let end = start + stride;
                    
                    if end <= self.matrix_quantized.len() {
                        if holo16 {
                            // 16-VIEW HOLOGRAPHIC
                            let mut sum_s = 0.0f64;
                            for v in 0..16 {
                                let d_start = start + v * self.quantized_dim;
                                let d_end = d_start + self.quantized_dim;
                                let q_start = v * self.quantized_dim;
                                let q_end = q_start + self.quantized_dim;
                                
                                if d_end <= self.matrix_quantized.len() && q_end <= q_bits.len() {
                                    let d = &self.matrix_quantized[d_start..d_end];
                                    let q = &q_bits[q_start..q_end];
                                    let sim = (1.0 - u8::hamming(d, q).unwrap_or(max_hamming) / max_hamming).max(0.0);
                                    sum_s += sim;
                                }
                            }
                            let holistic_sim = 0.0625 * sum_s;
                            let dist = (1.0 - holistic_sim) * max_hamming;
                            if dist < min_dist { min_dist = dist; }
                        } else {
                            // Standard Hamming
                            let doc_bits = &self.matrix_quantized[start..end];
                            if let Some(dist) = u8::hamming(doc_bits, q_bits) {
                                if dist < min_dist { min_dist = dist; }
                            }
                        }
                    }
                }
                (doc_idx, min_dist)
            }).collect()
        } else if holo16 {
            // DOC MODE + HOLOGRAPHIC
            (0..num_docs).into_par_iter().map(|doc_idx| {
                let start = doc_idx * stride;
                let mut sum_s = 0.0f64;
                
                for v in 0..16 {
                    let d_start = start + v * self.quantized_dim;
                    let d_end = d_start + self.quantized_dim;
                    let q_start = v * self.quantized_dim;
                    let q_end = q_start + self.quantized_dim;
                    
                    if d_end <= self.matrix_quantized.len() && q_end <= q_bits.len() {
                        let d = &self.matrix_quantized[d_start..d_end];
                        let q = &q_bits[q_start..q_end];
                        let sim = (1.0 - u8::hamming(d, q).unwrap_or(max_hamming) / max_hamming).max(0.0);
                        sum_s += sim;
                    }
                }
                let holistic_sim = 0.0625 * sum_s;
                let dist = (1.0 - holistic_sim) * max_hamming;
                (doc_idx, dist)
            }).collect()
        } else {
            // DOC MODE: Standard Hamming
            (0..num_docs).into_par_iter().map(|doc_idx| {
                let start = doc_idx * stride;
                let end = start + stride;
                
                let dist = if end <= self.matrix_quantized.len() {
                    let doc_bits = &self.matrix_quantized[start..end];
                    u8::hamming(doc_bits, q_bits).unwrap_or(f64::MAX)
                } else {
                    f64::MAX
                };
                (doc_idx, dist)
            }).collect()
        };
        
        candidates.sort_unstable_by(|a, b| 
            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
        );
        
        candidates.into_iter().take(limit).collect()
    }
    
    /// Hybrid candidate retrieval (semantic + lexical before filtering)
    fn hybrid_candidate_retrieval(
        &self,
        q_bits: &[u8],
        q_expanded: &AHashSet<String>,
        limit: usize,
    ) -> Vec<(usize, f64)> {
        let stride = self.bytes_per_passage;
        let max_hamming = (self.quantized_dim * 8) as f64;
        let holo16 = self.holographic_16view && q_bits.len() >= 16 * self.quantized_dim;
        let num_docs = self.doc_ids.len();
        
        let alpha_sem = self.hybrid_alpha_semantic as f64;
        let alpha_lex = self.hybrid_alpha_lexical as f64;
        
        let mut candidates: Vec<(usize, f64, f64)> = (0..num_docs)
            .into_par_iter()
            .map(|doc_idx| {
                // 1. LEXICAL SCORE - IDF-weighted overlap
                let (s_lexical, has_match) = if doc_idx < self.doc_word_sets_fast.len() {
                    let doc_words = &self.doc_word_sets_fast[doc_idx];
                    let mut idf_matched = 0.0f64;
                    let mut idf_total = 0.0f64;
                    let mut matched = false;
                    
                    for word in q_expanded.iter() {
                        let idf = *self.word_idf_fast.get(word).unwrap_or(&1.0) as f64;
                        idf_total += idf;
                        if self.word_to_id.get(word).map_or(false, |id| doc_words.contains(id)) {
                            matched = true;
                            idf_matched += idf;
                        }
                    }

                    let score = if idf_total > 0.0 { idf_matched / idf_total } else { 0.0 };
                    (score, matched)
                } else {
                    (0.0, false)
                };
                
                // 2. EARLY EXIT: Skip expensive Hamming if no lexical match
                if !has_match {
                    let estimated_combined = alpha_sem * 0.4;
                    let combined_dist = (1.0 - estimated_combined) * max_hamming;
                    return (doc_idx, f64::MAX, combined_dist);
                }

                // 3. SEMANTIC SCORE (Hamming) — GPU path or CPU path
                let semantic_dist = if let Some(ref gpu_dists) = self.gpu_precomputed_distances {
                    // GPU pre-computed: use directly (no CPU Hamming needed)
                    gpu_dists.get(doc_idx).copied().unwrap_or(u32::MAX) as f64
                } else if !self.passage_counts.is_empty() &&
                    self.passage_counts.get(doc_idx).map(|&c| c > 1).unwrap_or(false) {
                    let passage_count = self.passage_counts[doc_idx];
                    let passage_offset = self.passage_offsets[doc_idx];
                    let mut min_dist = f64::MAX;
                    
                    for p_idx in 0..passage_count {
                        let start = (passage_offset + p_idx) * stride;
                        let end = start + stride;
                        if end <= self.matrix_quantized.len() {
                            if holo16 {
                                let mut sum_s = 0.0f64;
                                for v in 0..16 {
                                    let d_start = start + v * self.quantized_dim;
                                    let d_end = d_start + self.quantized_dim;
                                    let q_start = v * self.quantized_dim;
                                    let q_end = q_start + self.quantized_dim;
                                    if d_end <= self.matrix_quantized.len() && q_end <= q_bits.len() {
                                        let d = &self.matrix_quantized[d_start..d_end];
                                        let q = &q_bits[q_start..q_end];
                                        let sim = (1.0 - u8::hamming(d, q).unwrap_or(max_hamming) / max_hamming).max(0.0);
                                        sum_s += sim;
                                    }
                                }
                                let holistic_sim = 0.0625 * sum_s;
                                let dist = (1.0 - holistic_sim) * max_hamming;
                                if dist < min_dist { min_dist = dist; }
                            } else {
                                let doc_bits = &self.matrix_quantized[start..end];
                                if let Some(dist) = u8::hamming(doc_bits, q_bits) {
                                    if dist < min_dist { min_dist = dist; }
                                }
                            }
                        }
                    }
                    min_dist
                } else if holo16 {
                    let start = doc_idx * stride;
                    let mut sum_s = 0.0f64;
                    for v in 0..16 {
                        let d_start = start + v * self.quantized_dim;
                        let d_end = d_start + self.quantized_dim;
                        let q_start = v * self.quantized_dim;
                        let q_end = q_start + self.quantized_dim;
                        if d_end <= self.matrix_quantized.len() && q_end <= q_bits.len() {
                            let d = &self.matrix_quantized[d_start..d_end];
                            let q = &q_bits[q_start..q_end];
                            let sim = (1.0 - u8::hamming(d, q).unwrap_or(max_hamming) / max_hamming).max(0.0);
                            sum_s += sim;
                        }
                    }
                    let holistic_sim = 0.0625 * sum_s;
                    (1.0 - holistic_sim) * max_hamming
                } else {
                    let start = doc_idx * stride;
                    let end = start + stride;
                    if end <= self.matrix_quantized.len() {
                        let doc_bits = &self.matrix_quantized[start..end];
                        u8::hamming(doc_bits, q_bits).unwrap_or(f64::MAX)
                    } else {
                        f64::MAX
                    }
                };
                
                let s_semantic = (1.0 - (semantic_dist / max_hamming)).max(0.0);
                
                // 4. COMBINED SCORE
                let combined_score = alpha_sem * s_semantic + alpha_lex * s_lexical;
                let combined_dist = (1.0 - combined_score) * max_hamming;
                
                (doc_idx, semantic_dist, combined_dist)
            })
            .collect();
        
        // Sort by COMBINED distance
        candidates.sort_unstable_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
        
        // Return (doc_idx, semantic_dist)
        candidates.into_iter()
            .take(limit)
            .map(|(doc_idx, semantic_dist, _)| (doc_idx, semantic_dist))
            .collect()
    }
    
    /// QJL asymmetric candidate retrieval (pure semantic path).
    /// Uses float query × binary docs for unbiased similarity estimation.
    fn asymmetric_candidate_retrieval(&self, q_centered: &[f32], limit: usize) -> Vec<(usize, f64)> {
        let max_hamming = (self.quantized_dim * 8) as f64;
        let num_docs = self.doc_ids.len();
        let dim = self.dim;
        let stride = self.bytes_per_passage;

        let mut candidates: Vec<(usize, f64)> = (0..num_docs).into_par_iter().map(|doc_idx| {
            let start = doc_idx * stride;
            let end = start + self.quantized_dim;
            if end <= self.matrix_quantized.len() {
                let doc_bits = &self.matrix_quantized[start..end];
                let dist = Self::asymmetric_distance(doc_bits, q_centered, dim, max_hamming);
                (doc_idx, dist)
            } else {
                (doc_idx, max_hamming)
            }
        }).collect();

        candidates.sort_unstable_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(limit);
        candidates
    }

    /// QJL asymmetric hybrid candidate retrieval.
    /// Combines asymmetric semantic distance with lexical IDF overlap.
    fn hybrid_candidate_retrieval_asymmetric(
        &self,
        _q_bits: &[u8], // kept for API compat, not used
        q_centered: &[f32],
        q_expanded: &AHashSet<String>,
        limit: usize,
    ) -> Vec<(usize, f64)> {
        let max_hamming = (self.quantized_dim * 8) as f64;
        let num_docs = self.doc_ids.len();
        let dim = self.dim;
        let stride = self.bytes_per_passage;

        let alpha_sem = self.hybrid_alpha_semantic as f64;
        let alpha_lex = self.hybrid_alpha_lexical as f64;

        let mut candidates: Vec<(usize, f64, f64)> = (0..num_docs)
            .into_par_iter()
            .map(|doc_idx| {
                // 1. LEXICAL SCORE - same as symmetric path
                let (s_lexical, has_match) = if doc_idx < self.doc_word_sets_fast.len() {
                    let doc_words = &self.doc_word_sets_fast[doc_idx];
                    let mut idf_matched = 0.0f64;
                    let mut idf_total = 0.0f64;
                    let mut matched = false;
                    for word in q_expanded.iter() {
                        let idf = *self.word_idf_fast.get(word).unwrap_or(&1.0) as f64;
                        idf_total += idf;
                        if self.word_to_id.get(word).map_or(false, |id| doc_words.contains(id)) {
                            matched = true;
                            idf_matched += idf;
                        }
                    }
                    let score = if idf_total > 0.0 { idf_matched / idf_total } else { 0.0 };
                    (score, matched)
                } else {
                    (0.0, false)
                };

                if !has_match {
                    let estimated_combined = alpha_sem * 0.4;
                    let combined_dist = (1.0 - estimated_combined) * max_hamming;
                    return (doc_idx, f64::MAX, combined_dist);
                }

                // 2. ASYMMETRIC SEMANTIC DISTANCE (QJL: float query × binary doc)
                let start = doc_idx * stride;
                let end = start + self.quantized_dim;
                let semantic_dist = if end <= self.matrix_quantized.len() {
                    let doc_bits = &self.matrix_quantized[start..end];
                    Self::asymmetric_distance(doc_bits, q_centered, dim, max_hamming)
                } else {
                    max_hamming
                };

                // 3. COMBINED SCORE
                let s_sem = (1.0 - semantic_dist / max_hamming).max(0.0);
                let combined = alpha_sem * s_sem + alpha_lex * s_lexical;
                let combined_dist = (1.0 - combined) * max_hamming;

                (doc_idx, semantic_dist, combined_dist)
            })
            .collect();

        candidates.sort_unstable_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
        candidates.into_iter()
            .take(limit)
            .map(|(doc_idx, semantic_dist, _)| (doc_idx, semantic_dist))
            .collect()
    }

    /// Pure lexical search (for codes/passkeys) — IDF-weighted
    fn search_pure_lexical_quantized(
        &self,
        _query_lower: &str,
        q_expanded: &AHashSet<String>,
        top_k: usize,
    ) -> Vec<(String, f32)> {
        let mut results: Vec<(String, f32)> = Vec::new();

        if q_expanded.is_empty() {
            return results;
        }

        // IDF-weight so rare words (names, codes) dominate over stopwords
        let total_query_idf: f32 = q_expanded.iter()
            .map(|w| *self.word_idf_fast.get(w).unwrap_or(&1.0))
            .sum::<f32>()
            .max(1.0);

        for (doc_idx, doc_id) in self.doc_ids.iter().enumerate() {
            if doc_idx >= self.doc_word_sets_fast.len() {
                continue;
            }
            let doc_words = &self.doc_word_sets_fast[doc_idx];

            let hit_idf: f32 = q_expanded.iter()
                .filter(|w| self.word_to_id.get(*w).map_or(false, |id| doc_words.contains(id)))
                .map(|w| *self.word_idf_fast.get(w).unwrap_or(&1.0))
                .sum();

            if hit_idf > 0.0 {
                results.push((doc_id.clone(), hit_idf / total_query_idf));
            }
        }

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.into_iter().take(top_k).collect()
    }
    
    /// Pure semantic search (Hamming only)
    fn search_pure_semantic_quantized(
        &self,
        survivors: &[(usize, f64)],
        max_hamming: f64,
        top_k: usize,
    ) -> Vec<(String, f32)> {
        let mut results: Vec<(String, f32)> = survivors.iter()
            .filter_map(|&(doc_idx, hamming_dist)| {
                self.doc_ids.get(doc_idx).map(|doc_id| {
                    let s_sem = (1.0 - (hamming_dist / max_hamming)).max(0.0) as f32;
                    (doc_id.clone(), s_sem)
                })
            })
            .collect();
        
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        results.into_iter().take(top_k).collect()
    }
    
    /// Full hybrid search — parallel scoring with fused overlap+TF-IDF,
    /// pre-computed phrase windows, and zero per-doc allocations.
    fn search_full_hybrid_quantized(
        &self,
        survivors: &[(usize, f64)],
        q_expanded: &AHashSet<String>,
        query_lower: &str,
        query_original: &str,
        max_hamming: f64,
        top_k: usize,
    ) -> Vec<(String, f32)> {
        // Rare words for entity TF-IDF weighting
        let rare_words: AHashSet<String> = q_expanded.iter()
            .filter(|w| *self.word_idf_fast.get(*w).unwrap_or(&0.0) > 2.5)
            .cloned()
            .collect();

        // ADAPTIVE QUERY PROFILE — continuous, not binary
        // Measures how "entity-like" vs "dialogue-like" a query is
        let query_idf_avg: f32 = q_expanded.iter()
            .map(|w| *self.word_idf_fast.get(w).unwrap_or(&0.5))
            .sum::<f32>() / q_expanded.len().max(1) as f32;
        let rare_ratio = rare_words.len() as f32 / q_expanded.len().max(1) as f32;

        // entity_signal: 0.0 = pure dialogue, 1.0 = entity-heavy
        // Also factors in rare word ratio for sharper discrimination
        let idf_signal = ((query_idf_avg - 1.2) / 1.2).clamp(0.0, 1.0);
        let entity_signal = (idf_signal * 0.7 + rare_ratio * 0.3).clamp(0.0, 1.0);

        // Dynamic scoring parameters based on entity_signal
        // IDF^1.0 (dialogue) → 1.15 (entity) — proven best balanced (v6)
        let idf_exponent = 1.0 + 0.15 * entity_signal;           // 1.0 → 1.15
        let tf_k1 = 1.5 + 1.5 * (1.0 - entity_signal);          // 1.5 (entity) → 3.0 (dialogue)
        let rare_weight = 0.60 + 0.15 * entity_signal;           // 0.60 → 0.75
        let common_weight = 1.0 - rare_weight;                    // 0.40 → 0.25

        let is_dialogue_query = entity_signal < 0.3;
        let dialogue_semantic_boost = (0.35 * (1.0 - entity_signal)).min(0.35);

        // Average doc length for BM25-style normalization
        let survivor_count = survivors.len().max(1);
        let total_len: usize = survivors.iter()
            .map(|&(doc_idx, _)| self.doc_word_sets_fast.get(doc_idx).map(|s| s.len()).unwrap_or(0))
            .sum();
        let avg_doc_len = total_len as f32 / survivor_count as f32;
        let b_param = 0.35f32;
        
        // Pre-compute query-level values (same for all docs)
        let query_words_raw: Vec<&str> = query_original.split_whitespace().collect();
        let idf_query: f32 = q_expanded.iter()
            .map(|w| *self.word_idf_fast.get(w).unwrap_or(&0.5))
            .sum::<f32>() / q_expanded.len().max(1) as f32;

        // Pre-compute phrase windows (expensive string ops, same for all docs)
        struct PhraseWindow {
            clean_words: Vec<String>,
            phrase: String,
            w_size: usize,
        }
        let mut phrase_windows: Vec<PhraseWindow> = Vec::new();
        if query_words_raw.len() >= 3 {
            for &w_size in &[4usize, 3] {
                if query_words_raw.len() < w_size { continue; }
                for window in query_words_raw.windows(w_size) {
                    let clean_words: Vec<String> = window.iter()
                        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
                        .collect();
                    let mut phrase_idf_sum = 0.0f32;
                    for w in &clean_words {
                        phrase_idf_sum += *self.word_idf_fast.get(w).unwrap_or(&0.5);
                    }
                    if phrase_idf_sum / (w_size as f32) < 2.2 { continue; }
                    let phrase = window.join(" ").to_lowercase();
                    if phrase.trim().len() < 10 { continue; }
                    phrase_windows.push(PhraseWindow { clean_words, phrase, w_size });
                }
            }
        }

        // Pre-split query for speaker boost
        let query_original_words: Vec<&str> = if is_dialogue_query {
            query_original.split_whitespace().collect()
        } else {
            Vec::new()
        };

        let mut results: Vec<(String, f32)> = Vec::with_capacity(survivors.len());

        for &(doc_idx, semantic_dist) in survivors {
            let doc_id = match self.doc_ids.get(doc_idx) {
                Some(id) => id.clone(),
                None => continue,
            };

            // 1. SEMANTIC SCORE (from Hamming distance)
            let s_sem = (1.0 - (semantic_dist / max_hamming)).max(0.0) as f32;
            
            let doc_words = match self.doc_word_sets_fast.get(doc_idx) {
                Some(w) => w,
                None => continue,
            };
            let doc_tf = self.doc_word_tf_fast.get(doc_idx);
            let doc_text = self.doc_texts_fast.get(doc_idx).map(|s| s.as_str()).unwrap_or("");
            
            // Exact match safety net
            if doc_text.contains(query_lower) {
                results.push((doc_id, 1.0));
                continue;
            }
            
            // 2. LEXICAL SCORE (Full TF-IDF)
            let overlap: AHashSet<String> = q_expanded.iter()
                .filter(|w| self.word_to_id.get(*w).map_or(false, |id| doc_words.contains(id)))
                .cloned()
                .collect();
            
            // Dynamic alpha based on IDF overlap
            let alpha = if overlap.is_empty() {
                1.0
            } else {
                let idf_overlap: f32 = overlap.iter()
                    .map(|w| *self.word_idf_fast.get(w).unwrap_or(&0.5))
                    .sum::<f32>() / overlap.len() as f32;
                let base_alpha = (1.0 - idf_overlap / idf_query.max(0.001)).clamp(0.0, 1.0);
                (base_alpha + dialogue_semantic_boost).min(1.0)
            };
            
            // ADAPTIVE TF-IDF — dynamic parameters from query profile
            let mut entity_score = 0.0f32;
            let mut common_score = 0.0f32;
            for word in &overlap {
                let tf = self.word_to_id.get(word)
                    .and_then(|id| doc_tf.and_then(|m| m.get(id)))
                    .copied().unwrap_or(1) as f32;
                let idf = *self.word_idf_fast.get(word).unwrap_or(&0.5);
                // Dynamic TF: saturates for entity queries, stays linear for dialogue
                let tf_component = (tf * tf_k1) / (tf + tf_k1);
                // Dynamic IDF: amplified for entity queries, standard for dialogue
                let tfidf = tf_component * idf.powf(idf_exponent);
                if rare_words.contains(word) {
                    entity_score += tfidf;
                } else {
                    common_score += tfidf;
                }
            }
            let total_tfidf = rare_weight * entity_score + common_weight * common_score;
            let token_ratio = (total_tfidf / 10.0).min(1.0);
            
            // Quadratic boost + BM25-style length normalization
            let boost = 1.0 + token_ratio.powi(2);
            let s_lex_raw = token_ratio * boost;
            let doc_len = doc_words.len() as f32;
            let length_norm = 1.0 - b_param + b_param * (doc_len / avg_doc_len.max(1.0));
            let s_lex = s_lex_raw / length_norm.max(0.5);
            
            // Phrase match boost (using pre-computed windows)
            let mut phrase_match_boost = 1.0f32;
            if !phrase_windows.is_empty() && !overlap.is_empty() {
                'outer: for pw in &phrase_windows {
                    if !pw.clean_words.iter().all(|w| self.doc_has_word(doc_idx, w)) { continue; }
                    if doc_text.contains(&pw.phrase) {
                        phrase_match_boost = if pw.w_size == 4 { 1.25 } else { 1.15 };
                        break 'outer;
                    }
                }
            }
            let s_lex_boosted = s_lex * phrase_match_boost;
            
            // Combined score
            let mut final_score = alpha * s_sem + (1.0 - alpha) * s_lex_boosted;
            
            // Speaker boost for dialogue queries
            if is_dialogue_query {
                for word in &query_original_words {
                    if word.len() >= 3 && word.chars().next().unwrap().is_uppercase() {
                        let word_lower = word.to_lowercase();
                        if doc_text.contains(*word) || self.doc_has_word(doc_idx, &word_lower) {
                            final_score *= 1.15;
                            break;
                        }
                    }
                }
            }
            
            results.push((doc_id, final_score));
        }
        
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // Span-density reranking on top candidates (tiebreaker, not primary signal)
        // Reranking all docs scans ~11MB of text; top 50 scans ~1.8MB — 6x faster
        let rerank_depth = 50.min(results.len());
        let q_words: Vec<String> = q_expanded.iter().cloned().collect();
        
        let mut champions: Vec<(String, f32)> = results.into_iter().take(rerank_depth).collect();
        
        champions.par_iter_mut().for_each(|(doc_id, score)| {
            if let Some(&doc_idx) = self.doc_id_to_idx.get(doc_id).map(|v| v) {
                let doc_idx = doc_idx as usize;
                if let Some(doc_text) = self.doc_texts_fast.get(doc_idx) {
                    let density_score = self.calculate_span_density_fast(doc_text, &q_words);
                    let boost_val = 1.0 + (density_score * 0.25);
                    *score *= boost_val;
                }
            }
        });
        
        champions.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        champions.into_iter().take(top_k).collect()
    }

    /// Calculate span density for reranking (sca_dropin style - exact alignment)
    #[inline(always)]
    fn calculate_span_density_fast(&self, doc_text: &str, query_terms: &[String]) -> f32 {
        if query_terms.len() < 2 { return 0.0; }
        
        let q_to_idx: AHashMap<String, usize> = query_terms.iter()
            .enumerate()
            .map(|(i, s)| (s.to_lowercase(), i))
            .collect();
        let query_idfs: Vec<f32> = query_terms.iter()
            .map(|t| *self.word_idf_fast.get(t).unwrap_or(&0.5))
            .collect();

        let mut positions: Vec<Vec<usize>> = vec![vec![]; query_terms.len()];
        for (pos, word) in doc_text.split_whitespace().enumerate() {
            let clean: String = word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
            if clean.is_empty() { continue; }
            if let Some(&q_idx) = q_to_idx.get(&clean) {
                positions[q_idx].push(pos);
            }
        }

        let mut active_indices = Vec::with_capacity(query_terms.len());
        let mut idf_sum = 0.0f32;
        for (idx, pos_list) in positions.iter().enumerate() {
            if !pos_list.is_empty() {
                active_indices.push(idx);
                idf_sum += query_idfs[idx];
            }
        }
        if active_indices.len() < 2 { return 0.0; }

        let active_positions: Vec<&Vec<usize>> = active_indices.iter().map(|&idx| &positions[idx]).collect();
        let n = active_indices.len();
        let mut cur = vec![0usize; n];
        let mut min_span = usize::MAX;

        loop {
            let mut lo = usize::MAX;
            let mut hi = 0usize;
            let mut advance_idx = 0;
            for (i, &ci) in cur.iter().enumerate() {
                let val = active_positions[i][ci];
                if val < lo { lo = val; advance_idx = i; }
                if val > hi { hi = val; }
            }
            let span = hi - lo + 1;
            if span < min_span { min_span = span; }
            cur[advance_idx] += 1;
            if cur[advance_idx] >= active_positions[advance_idx].len() { break; }
        }

        let term_weight = idf_sum.powf(1.5);
        let span_penalty = (min_span as f32).max(1.0).ln() + 1.0;
        term_weight / span_penalty
    }
    
    /// Extract focused passages from candidate documents using query term positions.
    ///
    /// Instead of blind 512-char chunks, this creates passages CENTERED on where
    /// query terms actually appear in the document. ART narrows candidates; this
    /// method extracts the most relevant text windows for those candidates.
    ///
    /// Returns: Vec<(doc_idx, Vec<passage_text>)> — focused passages per candidate.
    pub fn extract_focused_passages(
        &self,
        query_text: &str,
        candidate_indices: &[usize],
        window_chars: usize,   // passage window size (e.g. 512)
        max_passages: usize,   // max passages per doc (e.g. 5)
    ) -> Vec<(usize, Vec<String>)> {
        let q_words: Vec<String> = query_text
            .split_whitespace()
            .map(|w| w.to_lowercase().trim_end_matches(|c: char| c.is_ascii_punctuation()).to_string())
            .filter(|w| w.len() >= 3)
            .collect();

        if q_words.is_empty() {
            return Vec::new();
        }

        // IDF-weight query terms so we center on high-value matches
        let mut q_with_idf: Vec<(String, f32)> = q_words.iter()
            .map(|w| (w.clone(), *self.word_idf_fast.get(w).unwrap_or(&1.0)))
            .collect();
        q_with_idf.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut result = Vec::with_capacity(candidate_indices.len());

        for &doc_idx in candidate_indices {
            // Get full document text — try doc_texts_fast first (in-memory), then text_store
            let doc_text = if doc_idx < self.doc_texts_fast.len() {
                // doc_texts_fast is normalized word-level text (lowercase, punct-stripped)
                // For passage extraction we want the original text from text_store
                if let Some(doc_id) = self.doc_ids.get(doc_idx) {
                    self.get_text(doc_id).unwrap_or_else(|| self.doc_texts_fast[doc_idx].clone())
                } else {
                    continue;
                }
            } else if let Some(doc_id) = self.doc_ids.get(doc_idx) {
                match self.get_text(doc_id) {
                    Some(t) => t,
                    None => continue,
                }
            } else {
                continue;
            };

            let chars: Vec<char> = doc_text.chars().collect();
            let char_count = chars.len();
            if char_count == 0 { continue; }

            // Find character positions of query term matches
            let doc_lower = doc_text.to_lowercase();
            let mut match_positions: Vec<(usize, f32)> = Vec::new(); // (char_pos, idf_weight)

            for (word, idf) in &q_with_idf {
                let mut search_start = 0;
                while let Some(byte_pos) = doc_lower[search_start..].find(word.as_str()) {
                    let abs_byte_pos = search_start + byte_pos;
                    // Convert byte position to char position
                    let char_pos = doc_lower[..abs_byte_pos].chars().count();
                    match_positions.push((char_pos, *idf));
                    search_start = abs_byte_pos + word.len();
                    if search_start >= doc_lower.len() { break; }
                }
            }

            if match_positions.is_empty() {
                // Fallback: first window
                let end = std::cmp::min(window_chars, char_count);
                let passage: String = chars[0..end].iter().collect();
                if passage.trim().len() >= 50 {
                    result.push((doc_idx, vec![passage]));
                }
                continue;
            }

            // Sort by IDF weight (highest-value matches first)
            match_positions.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            // Create windows centered on match positions, merge overlapping
            let half_window = window_chars / 2;
            let mut windows: Vec<(usize, usize)> = Vec::new();

            for (pos, _idf) in &match_positions {
                let start = pos.saturating_sub(half_window);
                let end = std::cmp::min(start + window_chars, char_count);
                let start = if end == char_count { end.saturating_sub(window_chars) } else { start };

                // Check if overlaps with existing window — merge if so
                let mut merged = false;
                for w in windows.iter_mut() {
                    if start <= w.1 && end >= w.0 {
                        w.0 = std::cmp::min(w.0, start);
                        w.1 = std::cmp::max(w.1, end);
                        merged = true;
                        break;
                    }
                }
                if !merged {
                    windows.push((start, end));
                }

                if windows.len() >= max_passages {
                    break;
                }
            }

            // Extract passages from windows
            let passages: Vec<String> = windows.iter()
                .map(|&(s, e)| chars[s..e].iter().collect::<String>())
                .filter(|p| p.trim().len() >= 50)
                .collect();

            if !passages.is_empty() {
                result.push((doc_idx, passages));
            }
        }

        result
    }

    /// ART + word pre-filter: narrow candidates before scoring.
    /// Returns None if ART is empty (caller falls back to all docs).
    fn art_prefilter(&self, q_expanded: &AHashSet<String>, query_text: &str) -> Option<Vec<usize>> {
        if self.inverted_index.is_empty() {
            return None;
        }
        let mut candidates: AHashSet<usize> = AHashSet::new();

        // Word-level candidates (from word_inverted_fast, keyed by interned word-id)
        for word in q_expanded.iter() {
            if let Some(&wid) = self.word_to_id.get(word) {
                if let Some(doc_indices) = self.word_inverted_fast.get(&wid) {
                    candidates.extend(doc_indices.iter());
                }
            }
        }

        // BERT token-level candidates via ART (finer-grained)
        let q_tokens = self.get_tokens(query_text);
        for token_id in &q_tokens {
            let doc_ids_for_token = self.inverted_index.get(*token_id);
            for doc_id in doc_ids_for_token {
                if let Some(&idx) = self.doc_id_to_idx.get(&doc_id) {
                    candidates.insert(idx as usize);
                }
            }
        }

        if candidates.is_empty() { None } else { Some(candidates.into_iter().collect()) }
    }

    /// ART-filtered pure lexical search — iterates only ART candidates (or all docs if None).
    fn search_pure_lexical_filtered(
        &self,
        _query_lower: &str,
        q_expanded: &AHashSet<String>,
        top_k: usize,
        art_candidates: Option<&[usize]>,
    ) -> Vec<(String, f32)> {
        let mut results: Vec<(String, f32)> = Vec::new();

        if q_expanded.is_empty() {
            return results;
        }

        let total_query_idf: f32 = q_expanded.iter()
            .map(|w| *self.word_idf_fast.get(w).unwrap_or(&1.0))
            .sum::<f32>()
            .max(1.0);

        // Iterate ART candidates or fall back to all docs
        let doc_indices: Vec<usize> = match art_candidates {
            Some(candidates) => candidates.to_vec(),
            None => (0..self.doc_ids.len()).collect(),
        };

        for doc_idx in doc_indices {
            if doc_idx >= self.doc_word_sets_fast.len() {
                continue;
            }
            let doc_words = &self.doc_word_sets_fast[doc_idx];

            let hit_idf: f32 = q_expanded.iter()
                .filter(|w| self.word_to_id.get(*w).map_or(false, |id| doc_words.contains(id)))
                .map(|w| *self.word_idf_fast.get(w).unwrap_or(&1.0))
                .sum();

            if hit_idf > 0.0 {
                if let Some(doc_id) = self.doc_ids.get(doc_idx) {
                    results.push((doc_id.clone(), hit_idf / total_query_idf));
                }
            }
        }

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.into_iter().take(top_k).collect()
    }

    /// ART-filtered Hamming candidate retrieval — iterates only ART candidates (or all docs if None).
    fn hamming_filtered(&self, q_bits: &[u8], limit: usize, art_candidates: Option<&[usize]>) -> Vec<(usize, f64)> {
        // If no ART candidates, fall back to unfiltered
        if art_candidates.is_none() {
            return self.hamming_candidate_retrieval(q_bits, limit);
        }
        let candidates_list = art_candidates.unwrap();

        let stride = self.bytes_per_passage;
        let max_hamming = (self.quantized_dim * 8) as f64;
        let holo16 = self.holographic_16view && q_bits.len() >= 16 * self.quantized_dim;

        let mut candidates: Vec<(usize, f64)> = if !self.passage_counts.is_empty() &&
            self.passage_counts.iter().any(|&c| c > 1) {
            // PASSAGE MODE
            candidates_list.par_iter().map(|&doc_idx| {
                let passage_count = self.passage_counts.get(doc_idx).copied().unwrap_or(1);
                let passage_offset = self.passage_offsets.get(doc_idx).copied().unwrap_or(doc_idx);
                let mut min_dist = f64::MAX;

                for p_idx in 0..passage_count {
                    let start = (passage_offset + p_idx) * stride;
                    let end = start + stride;

                    if end <= self.matrix_quantized.len() {
                        if holo16 {
                            let mut sum_s = 0.0f64;
                            for v in 0..16 {
                                let d_start = start + v * self.quantized_dim;
                                let d_end = d_start + self.quantized_dim;
                                let q_start = v * self.quantized_dim;
                                let q_end = q_start + self.quantized_dim;
                                if d_end <= self.matrix_quantized.len() && q_end <= q_bits.len() {
                                    let d = &self.matrix_quantized[d_start..d_end];
                                    let q = &q_bits[q_start..q_end];
                                    let sim = (1.0 - u8::hamming(d, q).unwrap_or(max_hamming) / max_hamming).max(0.0);
                                    sum_s += sim;
                                }
                            }
                            let holistic_sim = 0.0625 * sum_s;
                            let dist = (1.0 - holistic_sim) * max_hamming;
                            if dist < min_dist { min_dist = dist; }
                        } else {
                            let doc_bits = &self.matrix_quantized[start..end];
                            if let Some(dist) = u8::hamming(doc_bits, q_bits) {
                                if dist < min_dist { min_dist = dist; }
                            }
                        }
                    }
                }
                (doc_idx, min_dist)
            }).collect()
        } else if holo16 {
            candidates_list.par_iter().map(|&doc_idx| {
                let start = doc_idx * stride;
                let mut sum_s = 0.0f64;
                for v in 0..16 {
                    let d_start = start + v * self.quantized_dim;
                    let d_end = d_start + self.quantized_dim;
                    let q_start = v * self.quantized_dim;
                    let q_end = q_start + self.quantized_dim;
                    if d_end <= self.matrix_quantized.len() && q_end <= q_bits.len() {
                        let d = &self.matrix_quantized[d_start..d_end];
                        let q = &q_bits[q_start..q_end];
                        let sim = (1.0 - u8::hamming(d, q).unwrap_or(max_hamming) / max_hamming).max(0.0);
                        sum_s += sim;
                    }
                }
                let holistic_sim = 0.0625 * sum_s;
                let dist = (1.0 - holistic_sim) * max_hamming;
                (doc_idx, dist)
            }).collect()
        } else {
            candidates_list.par_iter().map(|&doc_idx| {
                let start = doc_idx * stride;
                let end = start + stride;
                let dist = if end <= self.matrix_quantized.len() {
                    let doc_bits = &self.matrix_quantized[start..end];
                    u8::hamming(doc_bits, q_bits).unwrap_or(f64::MAX)
                } else {
                    f64::MAX
                };
                (doc_idx, dist)
            }).collect()
        };

        candidates.sort_unstable_by(|a, b|
            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
        );
        candidates.into_iter().take(limit).collect()
    }

    /// ART-filtered hybrid candidate retrieval — iterates only ART candidates (or all docs if None).
    fn hybrid_filtered(
        &self,
        q_bits: &[u8],
        q_expanded: &AHashSet<String>,
        art_candidates: Option<&[usize]>,
    ) -> Vec<(usize, f64)> {
        // If no ART candidates, fall back to unfiltered
        if art_candidates.is_none() {
            let limit = self.doc_ids.len();
            return self.hybrid_candidate_retrieval(q_bits, q_expanded, limit);
        }
        let candidates_list = art_candidates.unwrap();

        let stride = self.bytes_per_passage;
        let max_hamming = (self.quantized_dim * 8) as f64;
        let holo16 = self.holographic_16view && q_bits.len() >= 16 * self.quantized_dim;

        let alpha_sem = self.hybrid_alpha_semantic as f64;
        let alpha_lex = self.hybrid_alpha_lexical as f64;

        let mut candidates: Vec<(usize, f64, f64)> = candidates_list
            .par_iter()
            .map(|&doc_idx| {
                // 1. LEXICAL SCORE
                let (s_lexical, has_match) = if doc_idx < self.doc_word_sets_fast.len() {
                    let doc_words = &self.doc_word_sets_fast[doc_idx];
                    let mut idf_matched = 0.0f64;
                    let mut idf_total = 0.0f64;
                    let mut matched = false;

                    for word in q_expanded.iter() {
                        let idf = *self.word_idf_fast.get(word).unwrap_or(&1.0) as f64;
                        idf_total += idf;
                        if self.word_to_id.get(word).map_or(false, |id| doc_words.contains(id)) {
                            matched = true;
                            idf_matched += idf;
                        }
                    }

                    let score = if idf_total > 0.0 { idf_matched / idf_total } else { 0.0 };
                    (score, matched)
                } else {
                    (0.0, false)
                };

                // 2. EARLY EXIT
                if !has_match {
                    let estimated_combined = alpha_sem * 0.4;
                    let combined_dist = (1.0 - estimated_combined) * max_hamming;
                    return (doc_idx, f64::MAX, combined_dist);
                }

                // 3. SEMANTIC SCORE (Hamming)
                let semantic_dist = if !self.passage_counts.is_empty() &&
                    self.passage_counts.get(doc_idx).map(|&c| c > 1).unwrap_or(false) {
                    let passage_count = self.passage_counts[doc_idx];
                    let passage_offset = self.passage_offsets[doc_idx];
                    let mut min_dist = f64::MAX;

                    for p_idx in 0..passage_count {
                        let start = (passage_offset + p_idx) * stride;
                        let end = start + stride;
                        if end <= self.matrix_quantized.len() {
                            if holo16 {
                                let mut sum_s = 0.0f64;
                                for v in 0..16 {
                                    let d_start = start + v * self.quantized_dim;
                                    let d_end = d_start + self.quantized_dim;
                                    let q_start = v * self.quantized_dim;
                                    let q_end = q_start + self.quantized_dim;
                                    if d_end <= self.matrix_quantized.len() && q_end <= q_bits.len() {
                                        let d = &self.matrix_quantized[d_start..d_end];
                                        let q = &q_bits[q_start..q_end];
                                        let sim = (1.0 - u8::hamming(d, q).unwrap_or(max_hamming) / max_hamming).max(0.0);
                                        sum_s += sim;
                                    }
                                }
                                let holistic_sim = 0.0625 * sum_s;
                                let dist = (1.0 - holistic_sim) * max_hamming;
                                if dist < min_dist { min_dist = dist; }
                            } else {
                                let doc_bits = &self.matrix_quantized[start..end];
                                if let Some(dist) = u8::hamming(doc_bits, q_bits) {
                                    if dist < min_dist { min_dist = dist; }
                                }
                            }
                        }
                    }
                    min_dist
                } else if holo16 {
                    let start = doc_idx * stride;
                    let mut sum_s = 0.0f64;
                    for v in 0..16 {
                        let d_start = start + v * self.quantized_dim;
                        let d_end = d_start + self.quantized_dim;
                        let q_start = v * self.quantized_dim;
                        let q_end = q_start + self.quantized_dim;
                        if d_end <= self.matrix_quantized.len() && q_end <= q_bits.len() {
                            let d = &self.matrix_quantized[d_start..d_end];
                            let q = &q_bits[q_start..q_end];
                            let sim = (1.0 - u8::hamming(d, q).unwrap_or(max_hamming) / max_hamming).max(0.0);
                            sum_s += sim;
                        }
                    }
                    let holistic_sim = 0.0625 * sum_s;
                    (1.0 - holistic_sim) * max_hamming
                } else {
                    let start = doc_idx * stride;
                    let end = start + stride;
                    if end <= self.matrix_quantized.len() {
                        let doc_bits = &self.matrix_quantized[start..end];
                        u8::hamming(doc_bits, q_bits).unwrap_or(f64::MAX)
                    } else {
                        f64::MAX
                    }
                };

                let s_semantic = (1.0 - (semantic_dist / max_hamming)).max(0.0);
                let combined_score = alpha_sem * s_semantic + alpha_lex * s_lexical;
                let combined_dist = (1.0 - combined_score) * max_hamming;

                (doc_idx, semantic_dist, combined_dist)
            })
            .collect();

        candidates.sort_unstable_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
        candidates.into_iter()
            .map(|(doc_idx, semantic_dist, _)| (doc_idx, semantic_dist))
            .collect()
    }

    /// Unified quantized search (sca_dropin style) — with ART pre-filtering and no-embedding fallback
    pub fn search_unified_quantized(
        &self,
        query_emb: &[f32],
        query_text: &str,
        top_k: usize,
    ) -> Vec<(String, f32)> {
        let rescore_limit = 5000;
        let query_lower = query_text.to_lowercase();

        // Analyze query and determine route (or use forced route)
        let (mut route, _q_words, q_expanded, _idf_avg, _oov_ratio, _has_oov_code, _has_typo) =
            self.analyze_query_quantized(query_text);
        if let Some(forced) = self.force_route {
            route = forced;
        }

        // PATH A: PURE LEXICAL (Passkeys, Codes)
        if route == QueryRoute::PureLexical {
            return self.search_pure_lexical_quantized(&query_lower, &q_expanded, top_k);
        }

        // Quantize query for Hamming search (symmetric) or center for asymmetric (QJL)
        let q_bits = self.quantize_query(query_emb);
        let q_centered = if self.asymmetric_search { Some(self.center_query(query_emb)) } else { None };
        let max_hamming = (self.quantized_dim * 8) as f64;

        // If word index is empty (SCRM load without build_index), force PureSemantic.
        // The fingerprints are loaded — Hamming search works. No word index needed.
        let route = if self.word_to_id.is_empty() && route == QueryRoute::FullHybrid {
            QueryRoute::PureSemantic
        } else {
            route
        };

        // PATH B: PURE SEMANTIC (STS, Clustering)
        if route == QueryRoute::PureSemantic {
            if let Some(ref qc) = q_centered {
                let survivors = self.asymmetric_candidate_retrieval(qc, rescore_limit);
                return self.search_pure_semantic_quantized(&survivors, max_hamming, top_k);
            }
            let survivors = self.hamming_candidate_retrieval(&q_bits, rescore_limit);
            return self.search_pure_semantic_quantized(&survivors, max_hamming, top_k);
        }

        // PATH C: FULL HYBRID
        let hybrid_limit = self.doc_ids.len();
        let mut survivors = if let Some(ref qc) = q_centered {
            self.hybrid_candidate_retrieval_asymmetric(&q_bits, qc, &q_expanded, hybrid_limit)
        } else {
            self.hybrid_candidate_retrieval(&q_bits, &q_expanded, hybrid_limit)
        };
        // Smart gate: keep ALL docs with lexical match, gate semantic-only docs.
        // Docs with lexical overlap have semantic_dist < f64::MAX (early exit sets MAX).
        // This ensures entity/phrase matching can find any doc with word overlap.
        if survivors.len() > 50 {
            let lexical_count = survivors.iter()
                .filter(|&&(_, sd)| sd < f64::MAX)
                .count();
            // Keep all lexical matches + top semantic-only up to 50 total
            let keep = lexical_count.max(50);
            if keep < survivors.len() {
                survivors.truncate(keep);
            }
        }
        self.search_full_hybrid_quantized(&survivors, &q_expanded, &query_lower, query_text, max_hamming, top_k)
    }

    /// Timed search — returns (results, analyze_us, candidate_us, score_us, rerank_us)
    pub fn search_unified_timed(
        &self,
        query_emb: &[f32],
        query_text: &str,
        top_k: usize,
    ) -> (Vec<(String, f32)>, u64, u64, u64, u64) {
        let rescore_limit = 5000;
        let query_lower = query_text.to_lowercase();

        let t0 = crate::time_compat::Stopwatch::start();
        let (mut route, _q_words, q_expanded, _idf_avg, _oov_ratio, _has_oov_code, _has_typo) =
            self.analyze_query_quantized(query_text);
        if let Some(forced) = self.force_route {
            route = forced;
        }
        let analyze_us = t0.elapsed_micros();

        if route == QueryRoute::PureLexical {
            let r = self.search_pure_lexical_quantized(&query_lower, &q_expanded, top_k);
            return (r, analyze_us, 0, 0, 0);
        }

        let t1 = crate::time_compat::Stopwatch::start();
        let q_bits = self.quantize_query(query_emb);
        let max_hamming = (self.quantized_dim * 8) as f64;

        if route == QueryRoute::PureSemantic {
            let survivors = self.hamming_candidate_retrieval(&q_bits, rescore_limit);
            let cand_us = t1.elapsed_micros();
            let r = self.search_pure_semantic_quantized(&survivors, max_hamming, top_k);
            return (r, analyze_us, cand_us, 0, 0);
        }

        let hybrid_limit = self.doc_ids.len();
        let mut survivors = self.hybrid_candidate_retrieval(&q_bits, &q_expanded, hybrid_limit);
        // Smart gate: keep ALL docs with lexical match, gate semantic-only docs.
        // Docs with lexical overlap have semantic_dist < f64::MAX (early exit sets MAX).
        // This ensures entity/phrase matching can find any doc with word overlap.
        if survivors.len() > 50 {
            let lexical_count = survivors.iter()
                .filter(|&&(_, sd)| sd < f64::MAX)
                .count();
            // Keep all lexical matches + top semantic-only up to 50 total
            let keep = lexical_count.max(50);
            if keep < survivors.len() {
                survivors.truncate(keep);
            }
        }
        let cand_us = t1.elapsed_micros();

        let t2 = crate::time_compat::Stopwatch::start();
        let results = self.search_full_hybrid_quantized(&survivors, &q_expanded, &query_lower, query_text, max_hamming, top_k);
        let score_us = t2.elapsed_micros();

        (results, analyze_us, cand_us, score_us, 0)
    }

    /// Query analysis for quantized mode (sca_dropin style)
    fn analyze_query_quantized(&self, query_text: &str) -> (QueryRoute, Vec<String>, AHashSet<String>, f32, f32, bool, bool) {
        let q_words: Vec<String> = query_text
            .split_whitespace()
            .map(|s| {
                let lower = s.to_lowercase();
                lower.trim_end_matches(|c: char| c.is_ascii_punctuation()).to_string()
            })
            .filter(|s| s.len() >= 3)
            .collect();
        
        if q_words.is_empty() {
            return (QueryRoute::PureSemantic, q_words, AHashSet::new(), 0.5, 0.0, false, false);
        }
        
        let mut has_any_code = false;
        let mut has_typo = false;
        let mut known_word_count = 0;
        let mut total_idf = 0.0f32;
        let mut oov_count = 0;
        let mut q_expanded: AHashSet<String> = AHashSet::new();
        
        for word in &q_words {
            if looks_like_code(word) {
                has_any_code = true;
                total_idf += 5.0;
                known_word_count += 1;
                q_expanded.insert(word.clone());
                continue;
            }
            
            let in_idf = self.word_idf_fast.get(word);
            
            if in_idf.is_some() || self.word_to_id.contains_key(word) {
                known_word_count += 1;
                let idf = *in_idf.unwrap_or(&1.5);
                total_idf += idf;
                q_expanded.insert(word.clone());
            } else {
                oov_count += 1;

                // Try compound word splitting for code-intent words
                let mut compound_found = false;
                for &(compound, parts) in COMPOUND_SPLITS {
                    if word == compound {
                        for &part in parts {
                            if part.len() >= 3 {
                                q_expanded.insert(part.to_string());
                            }
                        }
                        q_expanded.insert(word.clone());
                        compound_found = true;
                        break;
                    }
                }
                if !compound_found {
                    // Try fuzzy expansion
                    let fuzzy_matches = self.fuzzy_expand_word_quantized(word, 5);
                    let valid_matches: Vec<String> = fuzzy_matches.iter()
                        .filter(|m| *m != word && self.word_to_id.contains_key(*m))
                        .cloned()
                        .collect();

                    if !valid_matches.is_empty() {
                        has_typo = true;
                        for m in valid_matches {
                            q_expanded.insert(m);
                        }
                    } else {
                        q_expanded.insert(word.clone());
                    }
                }
            }
        }

        // Calculate IDF average
        let content_words: Vec<f32> = q_expanded.iter()
            .filter_map(|w| self.word_idf_fast.get(w).copied())
            .filter(|&idf| idf >= 1.5)
            .collect();
        
        let idf_avg = if !content_words.is_empty() {
            content_words.iter().sum::<f32>() / content_words.len() as f32
        } else if has_any_code {
            10.0
        } else if known_word_count > 0 {
            total_idf / known_word_count as f32
        } else {
            1.0
        };
        
        let non_code_count = q_words.iter()
            .filter(|w| !looks_like_code(w))
            .count();
        let oov_ratio = if non_code_count > 0 {
            oov_count as f32 / non_code_count as f32
        } else {
            0.0
        };
        
        // Code intent detection
        let has_code_intent = q_words.iter().any(|w| {
            CODE_INTENT_WORDS.iter().any(|ci| w == *ci)
        });
        
        // Detect pure discourse
        let high_idf_count = q_expanded.iter()
            .filter(|w| *self.word_idf_fast.get(*w).unwrap_or(&0.0) > 2.5)
            .count();
        
        let is_short_discourse = q_words.len() <= 8 && !has_any_code &&
                                 !has_code_intent && idf_avg <= 1.2 &&
                                 high_idf_count == 0 && oov_ratio < 0.1;

        // Mostly-out-of-vocabulary, no code intent: the query words don't appear
        // in the corpus, so lexical (BM25) matching is hopeless and the FullHybrid
        // scorer would zero these docs out. The 1-bit fingerprint is the only
        // usable signal here, so route to PureSemantic. Without this, a purely
        // semantic query (e.g. "a doctor treating a sick person" against a doc
        // about "physician/patient") scored 0.0 once the corpus word index was
        // populated — only working by accident when the index was absent.
        let is_pure_semantic_oov = !has_any_code && !has_code_intent && oov_ratio >= 0.8;

        let route = if has_any_code || has_code_intent {
            QueryRoute::PureLexical
        } else if is_short_discourse || is_pure_semantic_oov {
            QueryRoute::PureSemantic
        } else {
            QueryRoute::FullHybrid
        };
        
        (route, q_words, q_expanded, idf_avg, oov_ratio, has_any_code, has_typo)
    }
    
    /// Fuzzy word expansion for quantized mode
    fn fuzzy_expand_word_quantized(&self, word: &str, top_k: usize) -> Vec<String> {
        let word_lower = word.to_lowercase();
        
        if self.word_to_id.contains_key(&word_lower) {
            return vec![word_lower];
        }
        
        let sx = self.get_soundex(&word_lower);
        let candidates = match self.phonetic_index_fast.get(&sx) {
            Some(c) => c,
            None => return vec![word_lower],
        };

        // candidates are interned word-ids; resolve each back to its String for levenshtein.
        let mut ranked: Vec<(String, usize)> = candidates.iter()
            .filter_map(|&id| self.word_of(id).map(|w| w.to_string()))
            .map(|c| { let d = self.levenshtein(&word_lower, &c); (c, d) })
            .filter(|(_, dist)| *dist <= 2)
            .collect();
        
        ranked.sort_by_key(|(_, dist)| *dist);
        
        let result: Vec<String> = ranked.into_iter()
            .take(top_k)
            .map(|(w, _)| w)
            .collect();
        
        if result.is_empty() {
            vec![word_lower]
        } else {
            result
        }
    }
    
    /// Check if quantized mode is enabled
    pub fn is_quantized_mode(&self) -> bool {
        self.quantized_mode
    }

    /// Debug info for diagnosing SCRM load issues.
    pub fn debug_quantized_state(&self) -> String {
        format!(
            "dim={} quantized_dim={} bytes_per_passage={} holo16={} matrix_len={} docs={} passages={:?} offsets={:?}",
            self.dim, self.quantized_dim, self.bytes_per_passage,
            self.holographic_16view, self.matrix_quantized.len(),
            self.doc_ids.len(),
            &self.passage_counts[..self.passage_counts.len().min(5)],
            &self.passage_offsets[..self.passage_offsets.len().min(5)],
        )
    }
    
    /// Get quantized stats
    pub fn get_quantized_stats(&self) -> (usize, usize, usize) {
        let total_passages: usize = self.passage_counts.iter().sum();
        (self.doc_ids.len(), total_passages, self.word_to_id.len())
    }

    /// Serialize the index to bytes (.said format).
    /// If full_text is true, includes document text for self-contained search_kv/search_exact.
    pub fn serialize_index(&self, full_text: bool) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();

        // --- Header (64 bytes) ---
        buf.extend_from_slice(b"SAID");                        // magic: 4 bytes
        buf.extend_from_slice(&2u16.to_le_bytes());            // version: 2 bytes
        let mut flags: u16 = 0;
        if full_text { flags |= 1; }
        if !self.matrix_quantized.is_empty() { flags |= 2; }
        if self.holographic_16view { flags |= 4; }
        buf.extend_from_slice(&flags.to_le_bytes());           // flags: 2 bytes
        buf.extend_from_slice(&(self.doc_ids.len() as u32).to_le_bytes()); // num_docs: 4 bytes
        // Placeholder for section offsets (6 x u64 = 48 bytes) + 4 bytes padding = 52 bytes
        let offsets_pos = buf.len();
        buf.extend_from_slice(&[0u8; 52]);
        // Total header: 4 + 2 + 2 + 4 + 52 = 64 bytes

        // --- Section 1: Doc IDs ---
        let sec1_offset = buf.len() as u64;
        for id in &self.doc_ids {
            let id_bytes = id.as_bytes();
            buf.extend_from_slice(&(id_bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(id_bytes);
        }

        // --- Section 2: ART inverted index ---
        let sec2_offset = buf.len() as u64;
        let art_items = self.inverted_index.items();
        buf.extend_from_slice(&(art_items.len() as u32).to_le_bytes());
        for (token_id, doc_ids_set) in &art_items {
            buf.extend_from_slice(&token_id.to_le_bytes());
            // Convert doc_id strings to indices
            let indices: Vec<u16> = doc_ids_set.iter()
                .filter_map(|did| self.doc_id_to_idx.get(did).map(|&i| i as u16))
                .collect();
            buf.extend_from_slice(&(indices.len() as u16).to_le_bytes());
            for idx in &indices {
                buf.extend_from_slice(&idx.to_le_bytes());
            }
        }

        // --- Section 3: Word IDF + inverted ---
        let sec3_offset = buf.len() as u64;
        buf.extend_from_slice(&(self.word_idf_fast.len() as u32).to_le_bytes());
        for (word, idf) in &self.word_idf_fast {
            let wb = word.as_bytes();
            buf.extend_from_slice(&(wb.len() as u16).to_le_bytes());
            buf.extend_from_slice(wb);
            buf.extend_from_slice(&idf.to_le_bytes());
            // Word inverted index entries (look up via the interned word-id)
            if let Some(doc_indices) = self.word_to_id.get(word).and_then(|wid| self.word_inverted_fast.get(wid)) {
                let indices: Vec<u16> = doc_indices.iter().map(|&i| i as u16).collect();
                buf.extend_from_slice(&(indices.len() as u16).to_le_bytes());
                for idx in &indices {
                    buf.extend_from_slice(&idx.to_le_bytes());
                }
            } else {
                buf.extend_from_slice(&0u16.to_le_bytes());
            }
        }

        // --- Section 4: matrix_quantized (if present) ---
        let sec4_offset = buf.len() as u64;
        if !self.matrix_quantized.is_empty() {
            buf.extend_from_slice(&(self.quantized_dim as u32).to_le_bytes());
            buf.extend_from_slice(&(self.matrix_quantized.len() as u32).to_le_bytes());
            buf.extend_from_slice(&self.matrix_quantized);
        }

        // --- Section 5: Passage metadata ---
        let sec5_offset = buf.len() as u64;
        buf.extend_from_slice(&(self.passage_counts.len() as u32).to_le_bytes());
        for &c in &self.passage_counts {
            buf.extend_from_slice(&(c as u16).to_le_bytes());
        }
        for &o in &self.passage_offsets {
            buf.extend_from_slice(&(o as u32).to_le_bytes());
        }

        // --- Section 6: Text data (if full_text) ---
        let sec6_offset = buf.len() as u64;
        if full_text {
            for id in &self.doc_ids {
                if let Some(idx) = self.doc_id_to_idx.get(id) {
                    let text = self.text_store.get_text(*idx).unwrap_or_default();
                    let tb = text.as_bytes();
                    buf.extend_from_slice(&(tb.len() as u32).to_le_bytes());
                    buf.extend_from_slice(tb);
                } else {
                    buf.extend_from_slice(&0u32.to_le_bytes());
                }
            }
        }

        // Write section offsets back into header
        let offsets = [sec1_offset, sec2_offset, sec3_offset, sec4_offset, sec5_offset, sec6_offset];
        for (i, &off) in offsets.iter().enumerate() {
            let pos = offsets_pos + i * 8;
            buf[pos..pos + 8].copy_from_slice(&off.to_le_bytes());
        }

        buf
    }

    /// Serialize ONLY the breadcrumbs — data that CANNOT be derived from original documents.
    /// Everything else (IDF, word index, phonetic, vocabulary) is rebuilt from text on preload.
    ///
    /// Format: [SCRM magic 4B] [version 2B] [flags 2B] [dim 4B]
    ///         [num_docs 4B] [corpus_mean] [doc_ids] [matrix_quantized]
    ///         [passage_counts] [passage_offsets] [holographic_scale 4B]
    pub fn serialize_breadcrumbs(&self) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();

        // Header
        buf.extend_from_slice(b"SCRM"); // SCA Recall Map
        buf.extend_from_slice(&3u16.to_le_bytes()); // version 3
        let mut flags: u16 = 0;
        if !self.matrix_quantized.is_empty() { flags |= 2; }
        if self.holographic_16view { flags |= 4; }
        buf.extend_from_slice(&flags.to_le_bytes());
        buf.extend_from_slice(&(self.dim as u32).to_le_bytes());
        buf.extend_from_slice(&(self.doc_ids.len() as u32).to_le_bytes());

        // Corpus mean (needed for quantize_query)
        buf.extend_from_slice(&(self.corpus_mean.len() as u32).to_le_bytes());
        for &v in &self.corpus_mean {
            buf.extend_from_slice(&v.to_le_bytes());
        }

        // Corpus std (for whitened binarization)
        buf.extend_from_slice(&(self.corpus_std.len() as u32).to_le_bytes());
        for &v in &self.corpus_std {
            buf.extend_from_slice(&v.to_le_bytes());
        }

        // Doc IDs
        for id in &self.doc_ids {
            let id_bytes = id.as_bytes();
            buf.extend_from_slice(&(id_bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(id_bytes);
        }

        // Quantized matrix (the fingerprints — 8 bytes per doc for 1-bit)
        buf.extend_from_slice(&(self.quantized_dim as u32).to_le_bytes());
        buf.extend_from_slice(&(self.matrix_quantized.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.matrix_quantized);

        // Passage metadata
        buf.extend_from_slice(&(self.passage_counts.len() as u32).to_le_bytes());
        for &c in &self.passage_counts {
            buf.extend_from_slice(&(c as u16).to_le_bytes());
        }
        for &o in &self.passage_offsets {
            buf.extend_from_slice(&(o as u32).to_le_bytes());
        }

        // Holographic scale
        buf.extend_from_slice(&self.holographic_scale.to_le_bytes());

        buf
    }

    /// Deserialize breadcrumbs (SCRM format). Word structures must be rebuilt from text.
    pub fn deserialize_breadcrumbs(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() < 16 || &data[0..4] != b"SCRM" {
            return Err("Invalid SCRM file".to_string());
        }

        let _version = u16::from_le_bytes([data[4], data[5]]);
        let flags = u16::from_le_bytes([data[6], data[7]]);
        let dim = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
        let num_docs = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
        let mut pos = 16;

        self.dim = dim;
        self.holographic_16view = flags & 4 != 0;
        self.quantized_mode = flags & 2 != 0;

        // Corpus mean
        if pos + 4 > data.len() { return Err("Truncated corpus mean len".to_string()); }
        let mean_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        self.corpus_mean.clear();
        for _ in 0..mean_len {
            if pos + 4 > data.len() { return Err("Truncated corpus mean".to_string()); }
            self.corpus_mean.push(f32::from_le_bytes(data[pos..pos+4].try_into().unwrap()));
            pos += 4;
        }

        // Corpus std (optional, backward compatible)
        self.corpus_std = vec![1.0; mean_len]; // default: no whitening
        if pos + 4 <= data.len() {
            let std_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
            if std_len == mean_len {
                pos += 4;
                self.corpus_std.clear();
                for _ in 0..std_len {
                    if pos + 4 > data.len() { break; }
                    self.corpus_std.push(f32::from_le_bytes(data[pos..pos+4].try_into().unwrap()));
                    pos += 4;
                }
            }
        }

        // Doc IDs
        self.doc_ids.clear();
        self.doc_id_to_idx.clear();
        for i in 0..num_docs {
            if pos + 2 > data.len() { return Err("Truncated doc IDs".to_string()); }
            let len = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
            pos += 2;
            if pos + len > data.len() { return Err("Truncated doc ID".to_string()); }
            let id = String::from_utf8_lossy(&data[pos..pos+len]).to_string();
            self.doc_id_to_idx.insert(id.clone(), i as u64);
            self.doc_ids.push(id);
            pos += len;
        }

        // Quantized matrix
        if pos + 8 > data.len() { return Err("Truncated matrix header".to_string()); }
        self.quantized_dim = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        let mat_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        if pos + mat_len > data.len() { return Err("Truncated matrix".to_string()); }
        self.matrix_quantized = data[pos..pos+mat_len].to_vec();
        pos += mat_len;
        self.bytes_per_passage = if self.holographic_16view { self.quantized_dim * 16 } else { self.quantized_dim };

        // Passage metadata
        if pos + 4 > data.len() { return Err("Truncated passage meta".to_string()); }
        let n_passages = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        self.passage_counts.clear();
        self.passage_offsets.clear();
        for _ in 0..n_passages {
            if pos + 2 > data.len() { break; }
            self.passage_counts.push(u16::from_le_bytes([data[pos], data[pos+1]]) as usize);
            pos += 2;
        }
        for _ in 0..n_passages {
            if pos + 4 > data.len() { break; }
            self.passage_offsets.push(u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize);
            pos += 4;
        }

        // Holographic scale
        if pos + 4 <= data.len() {
            self.holographic_scale = f32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
        }

        Ok(())
    }

    /// Deserialize index from bytes (.said format).
    pub fn deserialize_index(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() < 64 {
            return Err("File too small for .said header".to_string());
        }

        // Validate magic
        if &data[0..4] != b"SAID" {
            return Err("Invalid .said file: bad magic".to_string());
        }

        let version = u16::from_le_bytes([data[4], data[5]]);
        if version != 2 {
            return Err(format!("Unsupported .said version: {}", version));
        }

        let flags = u16::from_le_bytes([data[6], data[7]]);
        let has_full_text = flags & 1 != 0;
        let has_quantized = flags & 2 != 0;
        let has_16view = flags & 4 != 0;
        let num_docs = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;

        // Read section offsets
        let mut offsets = [0u64; 6];
        for i in 0..6 {
            let pos = 12 + i * 8;
            offsets[i] = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
        }

        // Clear existing state
        self.doc_ids.clear();
        self.inverted_index.clear();
        self.word_idf_fast.clear();
        self.word_inverted_fast.clear();
        self.word_vocab.clear();
        self.word_to_id.clear();
        self.doc_word_sets_fast.clear();
        self.doc_word_tf_fast.clear();
        self.doc_texts_fast.clear();
        self.phonetic_index_fast.clear();
        self.matrix_quantized.clear();
        self.passage_counts.clear();
        self.passage_offsets.clear();
        self.text_store.clear();
        self.doc_id_to_idx.clear();
        self.doc_token_sets.clear();

        // --- Section 1: Doc IDs ---
        let mut pos = offsets[0] as usize;
        for _ in 0..num_docs {
            if pos + 2 > data.len() { return Err("Truncated doc IDs".to_string()); }
            let len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            if pos + len > data.len() { return Err("Truncated doc ID".to_string()); }
            let id = String::from_utf8_lossy(&data[pos..pos + len]).to_string();
            self.doc_ids.push(id);
            pos += len;
        }

        // --- Section 2: ART inverted index ---
        pos = offsets[1] as usize;
        if pos + 4 > data.len() { return Err("Truncated ART section".to_string()); }
        let num_tokens = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        for _ in 0..num_tokens {
            if pos + 6 > data.len() { break; }
            let token_id = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
            pos += 4;
            let num_docs_for_token = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            for _ in 0..num_docs_for_token {
                if pos + 2 > data.len() { break; }
                let doc_idx = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                pos += 2;
                if let Some(doc_id) = self.doc_ids.get(doc_idx) {
                    self.inverted_index.add(token_id, doc_id, None);
                }
            }
        }

        // --- Section 3: Word IDF + inverted ---
        pos = offsets[2] as usize;
        if pos + 4 > data.len() { return Err("Truncated word section".to_string()); }
        let num_words = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        // Prepare doc_word_sets_fast
        self.doc_word_sets_fast.resize(num_docs, AHashSet::new());
        self.doc_word_tf_fast.resize(num_docs, AHashMap::new());
        self.doc_texts_fast.resize(num_docs, String::new());

        for _ in 0..num_words {
            if pos + 2 > data.len() { break; }
            let wlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            if pos + wlen > data.len() { break; }
            let word = String::from_utf8_lossy(&data[pos..pos + wlen]).to_string();
            pos += wlen;
            if pos + 4 > data.len() { break; }
            let idf = f32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
            pos += 4;
            self.word_idf_fast.insert(word.clone(), idf);

            if pos + 2 > data.len() { break; }
            let num_indices = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            let wid = self.intern_word(&word);
            let mut doc_indices = AHashSet::new();
            for _ in 0..num_indices {
                if pos + 2 > data.len() { break; }
                let doc_idx = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                pos += 2;
                doc_indices.insert(doc_idx);
                // Also populate doc_word_sets_fast (interned id)
                if doc_idx < num_docs {
                    self.doc_word_sets_fast[doc_idx].insert(wid);
                }
            }
            if !doc_indices.is_empty() {
                self.word_inverted_fast.insert(wid, doc_indices);
            }

            // Phonetic index
            let sx = self.get_soundex(&word);
            let wid = self.intern_word(&word);
            self.phonetic_index_fast
                .entry(sx)
                .or_insert_with(AHashSet::new)
                .insert(wid);
        }

        // --- Section 4: matrix_quantized ---
        if has_quantized {
            pos = offsets[3] as usize;
            if pos + 8 <= data.len() {
                let qdim = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                let qlen = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + qlen <= data.len() {
                    self.quantized_dim = qdim;
                    self.matrix_quantized = data[pos..pos + qlen].to_vec();
                    self.quantized_mode = true;
                    self.holographic_16view = has_16view;
                    self.bytes_per_passage = if has_16view { qdim * 16 } else { qdim };
                }
            }
        }

        // --- Section 5: Passage metadata ---
        pos = offsets[4] as usize;
        if pos + 4 <= data.len() {
            let np = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            for _ in 0..np {
                if pos + 2 > data.len() { break; }
                let c = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                pos += 2;
                self.passage_counts.push(c);
            }
            for _ in 0..np {
                if pos + 4 > data.len() { break; }
                let o = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                self.passage_offsets.push(o);
            }
        }

        // --- Section 6: Full text ---
        if has_full_text {
            pos = offsets[5] as usize;
            for (i, doc_id) in self.doc_ids.clone().iter().enumerate() {
                if pos + 4 > data.len() { break; }
                let tlen = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + tlen > data.len() { break; }
                let text = String::from_utf8_lossy(&data[pos..pos + tlen]).to_string();
                pos += tlen;
                let idx = self.text_store.append_text(&text).unwrap_or(0);
                self.doc_id_to_idx.insert(doc_id.clone(), idx);

                // Also rebuild doc_token_sets from text for search_exact/search_kv
                let tokens = self.get_tokens(&text);
                let mut bitmap = RoaringBitmap::new();
                for token_id in &tokens {
                    bitmap.insert(*token_id);
                }
                self.doc_token_sets.insert(doc_id.clone(), bitmap);

                // Rebuild doc_texts_fast
                if i < self.doc_texts_fast.len() {
                    self.doc_texts_fast[i] = text.split_whitespace()
                        .map(|w| w.to_lowercase().trim_end_matches(|c: char| c.is_ascii_punctuation()).to_string())
                        .collect::<Vec<_>>()
                        .join(" ");
                }
            }
        }

        self.rerank_depth = self.doc_ids.len();
        Ok(())
    }
}

/// Dot product of two vectors
fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Free function: Code/passkey detection (sca_dropin alignment)
fn looks_like_code(word: &str) -> bool {
    // Early exit for common false positives
    if word.contains('/') || word.contains('-') {
        return false;
    }
    if word.contains('$') || word.contains('€') || word.contains('£') || word.contains(',') {
        return false;
    }
    if word.starts_with('(') || word.starts_with('[') {
        return false;
    }
    if word.chars().any(|c| c == '±' || c == '×' || c == '÷') {
        return false;
    }

    let clean: String = word.chars()
        .filter(|c| c.is_alphanumeric())
        .collect();

    if clean.len() < 5 || clean.len() > 15 {
        return false;
    }

    let lower = clean.to_lowercase();
    if lower.ends_with("st") || lower.ends_with("nd") ||
       lower.ends_with("rd") || lower.ends_with("th") {
        let prefix = &lower[..lower.len()-2];
        if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
    }

    let measurement_suffixes = ["kg", "km", "cm", "mm", "ml", "mg", "gb", "mb", "kb", "hz", "bn", "mn", "bln"];
    for suffix in measurement_suffixes {
        if lower.ends_with(suffix) {
            let prefix = &lower[..lower.len() - suffix.len()];
            if prefix.chars().all(|c| c.is_ascii_digit() || c == '.') {
                return false;
            }
        }
    }

    let chars: Vec<char> = clean.chars().collect();
    let digit_count = chars.iter().filter(|c| c.is_ascii_digit()).count();
    let letter_count = chars.iter().filter(|c| c.is_ascii_alphabetic()).count();

    // Pure digits, length 5-10 (passkeys)
    if letter_count == 0 && digit_count >= 5 && digit_count <= 10 {
        return true;
    }

    false
}

// ═══════════════════════════════════════════════════════════════════════════════
// FILLER POOL: For ultra-fast document generation (testing)
// ═══════════════════════════════════════════════════════════════════════════════

/// Create pre-indexed filler pool for fast document generation.
/// Uses deterministic pseudo-random generation for reproducibility.
#[allow(dead_code)]
pub fn create_filler_pool(num_segments: usize, tokens_per_segment: usize) -> (Vec<String>, Vec<HashSet<String>>) {
    let legal_words = vec![
        "whereas", "therefore", "notwithstanding", "herein", "thereof",
        "aforesaid", "party", "parties", "agreement", "contract",
        "provision", "clause", "section", "article", "paragraph",
    ];
    
    let mut segments = Vec::new();
    let mut segment_tokens = Vec::new();
    
    let words_per_segment = tokens_per_segment / 13; // ~1.3 tokens per word
    let sentences_per_segment = words_per_segment / 20;
    
    // Simple deterministic pseudo-random using segment index as seed
    let mut seed: u64 = 12345;
    
    for seg_idx in 0..num_segments {
        let mut sentences = Vec::new();
        for sent_idx in 0..sentences_per_segment {
            // Update seed deterministically
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345 + seg_idx as u64 + sent_idx as u64);
            let num_words = 15 + ((seed / 7) % 11) as usize; // 15-25 words
            
            let sentence: Vec<&str> = (0..num_words)
                .map(|i| {
                    seed = seed.wrapping_mul(1103515245).wrapping_add(i as u64);
                    legal_words[(seed as usize) % legal_words.len()]
                })
                .collect();
            
            let mut s = sentence.join(" ");
            s.push('.');
            
            // Capitalize first letter
            let first_upper: String = s.chars().next()
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_default();
            s = format!("{}{}", first_upper, &s[1..]);
            
            sentences.push(s);
        }
        let text = sentences.join(" ");
        
        // Simple tokenize for token set
        let re = Regex::new(r"\w+").unwrap();
        let tokens: HashSet<String> = re.find_iter(&text.to_lowercase())
            .map(|m| m.as_str().to_string())
            .collect();
        
        segments.push(text);
        segment_tokens.push(tokens);
    }
    
    (segments, segment_tokens)
}

// ═══════════════════════════════════════════════════════════════════════════════
// This gets integrated into LAM class - users just do: from lam import LAM
// ═══════════════════════════════════════════════════════════════════════════════


