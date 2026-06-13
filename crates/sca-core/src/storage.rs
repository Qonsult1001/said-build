//! Simple text storage for CrystallineCore.
//!
//! InMemoryTextStore: Vec<String> backed storage with u64 index lookup.
//! TextStorage: Enum wrapping InMemory (future: Persistent/Mmap).

use std::io;

/// Simple in-memory text store — Vec<String> with u64 index.
#[derive(Clone, Default)]
pub struct InMemoryTextStore {
    texts: Vec<String>,
}

impl InMemoryTextStore {
    pub fn new() -> Self {
        Self { texts: Vec::new() }
    }

    pub fn append_text(&mut self, text: &str) -> io::Result<u64> {
        let idx = self.texts.len() as u64;
        self.texts.push(text.to_string());
        Ok(idx)
    }

    pub fn get_text(&self, idx: u64) -> io::Result<String> {
        self.texts.get(idx as usize)
            .cloned()
            .ok_or_else(|| io::Error::new(
                io::ErrorKind::NotFound,
                format!("Document index {} not found", idx),
            ))
    }

    pub fn get_text_ref(&self, idx: u64) -> Option<&str> {
        self.texts.get(idx as usize).map(|s| s.as_str())
    }

    pub fn doc_count(&self) -> usize {
        self.texts.len()
    }

    pub fn total_bytes(&self) -> u64 {
        self.texts.iter().map(|s| s.len() as u64).sum()
    }

    pub fn clear(&mut self) {
        self.texts.clear();
    }
}

/// Text storage abstraction.
#[derive(Clone)]
pub enum TextStorage {
    /// In-memory storage (default)
    InMemory(InMemoryTextStore),
}

impl TextStorage {
    /// Create ephemeral in-memory storage.
    pub fn new_ephemeral() -> Self {
        TextStorage::InMemory(InMemoryTextStore::new())
    }

    pub fn append_text(&mut self, text: &str) -> io::Result<u64> {
        match self {
            TextStorage::InMemory(store) => store.append_text(text),
        }
    }

    pub fn get_text(&self, idx: u64) -> io::Result<String> {
        match self {
            TextStorage::InMemory(store) => store.get_text(idx),
        }
    }

    pub fn doc_count(&self) -> usize {
        match self {
            TextStorage::InMemory(store) => store.doc_count(),
        }
    }

    pub fn total_bytes(&self) -> u64 {
        match self {
            TextStorage::InMemory(store) => store.total_bytes(),
        }
    }

    pub fn is_persistent(&self) -> bool {
        false
    }

    pub fn clear(&mut self) {
        match self {
            TextStorage::InMemory(store) => store.clear(),
        }
    }
}
