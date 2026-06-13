//! Integration tests showing how LLM/embedding providers plug in SCA.
//!
//! API matches LAM/MTEB naming conventions:
//!   index()  — matches LAM.index(doc_id, text) / MTEB SearchProtocol.index()
//!   search() — matches LAM.search(query) / MTEB SearchProtocol.search()
//!   encode() — matches LAM.encode(texts) (optional, SCA uses HDC not neural)
//!   save()   — matches LAM.save_index(path)
//!   load()   — matches LAM.load_index(path)

use sca_core::SCAPlugin;

// ============================================================================
// Pattern 1: Text-only — no embedding model needed
// ============================================================================

#[test]
fn pattern_text_only() {
    let mut sca = SCAPlugin::new();
    sca.index("rust", "Rust is a systems programming language focused on memory safety and ownership");
    sca.index("python", "Python excels at data science and machine learning with neural networks");
    sca.index("docker", "Docker containers enable cloud native microservice deployment");

    let hits = sca.search("memory safe programming language", 3);
    assert!(!hits.is_empty());
    assert_eq!(hits[0].id, "rust");

    let hits = sca.search("machine learning data science", 3);
    assert!(!hits.is_empty());
    assert_eq!(hits[0].id, "python");

    let hits = sca.search("container orchestration cloud", 3);
    assert!(!hits.is_empty());
    assert_eq!(hits[0].id, "docker");
}

// ============================================================================
// Pattern 2: With embeddings from any model
// ============================================================================

#[test]
fn pattern_with_embeddings() {
    let mut sca = SCAPlugin::new();

    // Simulate embeddings from any model (OpenAI, BGE, LAM, etc.)
    let rust_emb = fake_embedding(384, 42);
    let python_emb = fake_embedding(384, 99);

    sca.index_with_embedding("rust", "Rust is a systems language", &rust_emb);
    sca.index_with_embedding("python", "Python for machine learning", &python_emb);

    let query_emb = fake_embedding(384, 42); // Close to rust_emb
    let hits = sca.search_with_embedding("systems programming", &query_emb, 3);
    assert!(!hits.is_empty());
    assert_eq!(hits[0].id, "rust");
}

// ============================================================================
// Pattern 3: Persistence — save and load (matches LAM.save_index/load_index)
// ============================================================================

#[test]
fn pattern_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sca");

    {
        let mut sca = SCAPlugin::new();
        sca.index("doc1", "First document about Rust programming");
        sca.index("doc2", "Second document about Python scripting");
        sca.index("doc3", "Third document about cloud deployment");
        sca.save(&path).unwrap();
        assert_eq!(sca.len(), 3);
    }

    {
        let mut sca = SCAPlugin::load(&path).unwrap();
        assert_eq!(sca.len(), 3);

        let hits = sca.search("Rust programming", 1);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].id, "doc1");

        let hits = sca.search("cloud deployment", 1);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].id, "doc3");
    }
}

// ============================================================================
// Pattern 4: Bulk indexing (1000+ docs)
// ============================================================================

#[test]
fn pattern_bulk_indexing() {
    let mut sca = SCAPlugin::new();

    let topics = [
        "Rust systems programming memory safety borrow checker ownership",
        "Python machine learning tensorflow pytorch neural networks",
        "Docker Kubernetes containers cloud deployment orchestration",
        "JavaScript React Angular Vue TypeScript frontend web",
        "PostgreSQL database SQL queries indexing transactions ACID",
    ];

    let docs: Vec<(&str, &str)> = (0..1000)
        .map(|i| {
            let topic = topics[i % topics.len()];
            let id: &str = Box::leak(format!("doc_{}", i).into_boxed_str());
            let text: &str = Box::leak(format!("{} document {} with extended content", topic, i).into_boxed_str());
            (id, text)
        })
        .collect();

    sca.index_many(&docs);
    assert_eq!(sca.len(), 1000);

    let hits = sca.search("Rust borrow checker", 10);
    assert!(!hits.is_empty());
    for hit in &hits {
        let doc_num: usize = hit.id.strip_prefix("doc_").unwrap().parse().unwrap();
        assert_eq!(doc_num % 5, 0, "Expected Rust doc, got doc_{}", doc_num);
    }
}

// ============================================================================
// Pattern 5: OpenAI / any API integration (matches their encode→index→search)
// ============================================================================

#[test]
fn pattern_openai_style_integration() {
    // Python equivalent:
    //   import openai
    //   from sca_recall import SCA
    //
    //   client = openai.Client()
    //   sca = SCA()
    //
    //   for doc in documents:
    //       emb = client.embeddings.create(input=doc.text, model="text-embedding-3-small")
    //       sca.index(doc.id, doc.text, embedding=emb.data[0].embedding)
    //
    //   query_emb = client.embeddings.create(input=query, model="text-embedding-3-small")
    //   hits = sca.search(query, embedding=query_emb.data[0].embedding, top_k=10)

    let mut sca = SCAPlugin::new();
    sca.index_with_embedding("doc1", "Rust memory safety", &fake_embedding(1536, 1));
    sca.index_with_embedding("doc2", "Python machine learning", &fake_embedding(1536, 2));

    let hits = sca.search_with_embedding("safe programming", &fake_embedding(1536, 1), 2);
    assert!(!hits.is_empty());
    assert_eq!(hits[0].id, "doc1");
}

// ============================================================================
// Pattern 6: LAM integration (matches LAM.encode→SCA.index→SCA.search)
// ============================================================================

#[test]
fn pattern_lam_integration() {
    // Python equivalent:
    //   from said_lam import LAM
    //   from sca_recall import SCA
    //
    //   lam = LAM()
    //   sca = SCA()
    //
    //   # Index
    //   for doc in documents:
    //       emb = lam.encode([doc.text])[0].tolist()
    //       sca.index(doc.id, doc.text, embedding=emb)
    //
    //   # Search
    //   query_emb = lam.encode([query])[0].tolist()
    //   hits = sca.search(query, embedding=query_emb, top_k=10)

    let mut sca = SCAPlugin::new();
    sca.index_with_embedding("rust", "Rust is a systems language", &fake_embedding(384, 42));
    sca.index_with_embedding("python", "Python for ML", &fake_embedding(384, 99));

    let hits = sca.search_with_embedding("systems programming", &fake_embedding(384, 42), 2);
    assert!(!hits.is_empty());
    assert_eq!(hits[0].id, "rust");
}

// ============================================================================
// Pattern 7: Stats (matches LAM.stats())
// ============================================================================

#[test]
fn pattern_stats() {
    let mut sca = SCAPlugin::new();
    assert!(sca.is_empty());

    sca.index("doc1", "Hello world of programming");
    assert_eq!(sca.len(), 1);
    assert!(!sca.is_empty());
    assert!(sca.contains("doc1"));
    assert!(!sca.contains("doc2"));
    assert!(sca.token_count() > 0);
    assert!(sca.vocab_size() > 0);
    assert_eq!(sca.get_document("doc1"), Some("Hello world of programming"));

    let stats = sca.stats();
    assert_eq!(stats.docs, 1);
    assert!(stats.tokens > 0);
    assert!(stats.vocab > 0);
}

// ============================================================================
// Helpers
// ============================================================================

fn fake_embedding(dim: usize, seed: u64) -> Vec<f32> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    (0..dim)
        .map(|i| {
            let mut h = DefaultHasher::new();
            (seed, i as u64).hash(&mut h);
            (h.finish() as f32 / u64::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}
