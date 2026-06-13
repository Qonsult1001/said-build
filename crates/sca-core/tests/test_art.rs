use sca_core::art::{ART, ARTDict};

#[test]
fn test_art_insert_and_get() {
    let mut art = ART::new();
    art.insert(42, "doc1", Some(0));
    art.insert(42, "doc2", Some(5));
    art.insert(99, "doc1", None);

    let docs = art.get(42);
    assert!(docs.contains("doc1"));
    assert!(docs.contains("doc2"));
    assert_eq!(docs.len(), 2);

    let docs99 = art.get(99);
    assert!(docs99.contains("doc1"));
    assert_eq!(docs99.len(), 1);
}

#[test]
fn test_art_get_nonexistent() {
    let art = ART::new();
    let docs = art.get(12345);
    assert!(docs.is_empty());
}

#[test]
fn test_art_positions() {
    let mut art = ART::new();
    art.insert(42, "doc1", Some(0));
    art.insert(42, "doc1", Some(10));
    art.insert(42, "doc1", Some(25));

    let positions = art.get_positions(42);
    let doc1_pos = positions.get("doc1").unwrap();
    assert_eq!(doc1_pos, &vec![0, 10, 25]);
}

#[test]
fn test_art_remove() {
    let mut art = ART::new();
    art.insert(42, "doc1", None);
    art.insert(42, "doc2", None);

    assert_eq!(art.get_doc_freq(42), 2);
    assert!(art.remove(42, "doc1"));
    assert_eq!(art.get_doc_freq(42), 1);

    let docs = art.get(42);
    assert!(!docs.contains("doc1"));
    assert!(docs.contains("doc2"));
}

#[test]
fn test_art_remove_last_cleans_up() {
    let mut art = ART::new();
    art.insert(42, "doc1", None);
    assert_eq!(art.len(), 1);
    art.remove(42, "doc1");
    assert_eq!(art.len(), 0);
    assert!(art.get(42).is_empty());
}

#[test]
fn test_art_remove_nonexistent() {
    let mut art = ART::new();
    assert!(!art.remove(42, "doc1"));
}

#[test]
fn test_art_len_and_postings() {
    let mut art = ART::new();
    assert!(art.is_empty());
    art.insert(1, "doc1", None);
    art.insert(1, "doc2", None);
    art.insert(2, "doc1", None);
    assert_eq!(art.len(), 2);
    assert_eq!(art.postings_count(), 3);
}

#[test]
fn test_art_iter_all() {
    let mut art = ART::new();
    art.insert(10, "doc1", None);
    art.insert(20, "doc2", None);
    art.insert(10, "doc3", None);
    let all = art.iter_all();
    assert_eq!(all.len(), 2);
    let token10 = all.iter().find(|(id, _)| *id == 10).unwrap();
    assert_eq!(token10.1.len(), 2);
}

#[test]
fn test_art_clear() {
    let mut art = ART::new();
    art.insert(1, "doc1", None);
    art.insert(2, "doc2", None);
    art.clear();
    assert!(art.is_empty());
    assert_eq!(art.len(), 0);
    assert_eq!(art.postings_count(), 0);
}

#[test]
fn test_art_many_tokens() {
    let mut art = ART::new();
    for i in 0..100u32 {
        art.insert(i, &format!("doc{}", i % 10), Some(i as usize));
    }
    assert_eq!(art.len(), 100);
    let docs = art.get(50);
    assert!(docs.contains("doc0"));
}

#[test]
fn test_art_prefix_search() {
    let mut art = ART::new();
    let base: u32 = 0x0100_0000;
    art.insert(base, "doc1", None);
    art.insert(base + 1, "doc2", None);
    art.insert(base + 256, "doc3", None);
    let results = art.search_prefix(base, 1);
    assert!(results.len() >= 2);
}

#[test]
fn test_artdict_wrapper() {
    let mut dict = ARTDict::new();
    assert!(dict.is_empty());
    dict.add(42, "doc1", Some(0));
    dict.add(42, "doc2", None);
    assert!(dict.contains(42));
    assert!(!dict.contains(99));
    assert_eq!(dict.len(), 1);
    assert_eq!(dict.get_doc_freq(42), 2);
    dict.discard(42, "doc1");
    assert_eq!(dict.get_doc_freq(42), 1);
}
