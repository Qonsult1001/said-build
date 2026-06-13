//! Symbol index — O(1) lookup of code symbols by name.
//!
//! Built alongside the trigram index at compact time. Maps:
//!
//! ```text
//! symbol_name -> [
//!   (doc_index, kind, start_line, end_line),
//!   ...
//! ]
//! ```
//!
//! Where `doc_index` references the `trigram_doc_ids` list on `SaidFile`,
//! so this index shares storage with TRGM's doc table.
//!
//! ## Use cases
//!
//! - `said sym compact_block_dict` -> "fn at crates/sca-core/src/frames.rs:383"
//! - `said sym new`                 -> list all `pub fn new()` in the project
//! - `said sym FrameStore`          -> struct definition + all its impl methods
//!
//! ## Why a flat table
//!
//! A radix-trie would be faster for prefix matches but ~10x more code. For
//! code corpora the symbol count is small (~30K for SAID-ECHO) and a sorted
//! Vec + binary search is both simpler and cache-friendly.

use std::collections::BTreeMap;

/// Kind of a code symbol — packed into a u8 for storage.
///
/// Tree-sitter emits a string kind per chunk (e.g. `function_item`,
/// `struct_item`). We map those to this compact enum to stay fast and small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SymbolKind {
    Function = 0,
    Struct = 1,
    Enum = 2,
    Trait = 3,
    Impl = 4,
    Class = 5,
    Method = 6,
    Type = 7,
    Const = 8,
    Other = 9,
    // SQL
    Table = 10,
    View = 11,
    Procedure = 12,
    Trigger = 13,
    Index = 14,
}

impl SymbolKind {
    pub fn from_ts_kind(ts_kind: &str) -> Self {
        match ts_kind {
            // Rust
            "function_item" => SymbolKind::Function,
            "struct_item" => SymbolKind::Struct,
            "enum_item" => SymbolKind::Enum,
            "trait_item" => SymbolKind::Trait,
            "impl_item" => SymbolKind::Impl,
            "type_item" => SymbolKind::Type,
            "const_item" | "static_item" => SymbolKind::Const,
            // Python
            "function_definition" => SymbolKind::Function,
            "class_definition" => SymbolKind::Class,
            "decorated_definition" => SymbolKind::Function,
            // JS/TS
            "function_declaration" | "arrow_function" => SymbolKind::Function,
            "class_declaration" => SymbolKind::Class,
            "method_definition" | "method_declaration" => SymbolKind::Method,
            "interface_declaration" => SymbolKind::Trait,
            "type_declaration" => SymbolKind::Type,
            "lexical_declaration" => SymbolKind::Const,
            // Go / Java / C#
            "constructor_declaration" => SymbolKind::Method,
            "enum_declaration" => SymbolKind::Enum,
            // SQL / T-SQL
            "create_table" | "alter_table" | "drop_table" => SymbolKind::Table,
            "create_view" | "alter_view" | "create_materialized_view" => SymbolKind::View,
            "create_procedure" | "alter_procedure" | "create_function" | "alter_function" => SymbolKind::Procedure,
            "create_trigger" => SymbolKind::Trigger,
            "create_index" | "alter_index" => SymbolKind::Index,
            "create_schema" | "create_type" | "create_role" => SymbolKind::Type,
            _ => SymbolKind::Other,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "fn",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Impl => "impl",
            SymbolKind::Class => "class",
            SymbolKind::Method => "method",
            SymbolKind::Type => "type",
            SymbolKind::Const => "const",
            SymbolKind::Other => "?",
            SymbolKind::Table => "table",
            SymbolKind::View => "view",
            SymbolKind::Procedure => "proc",
            SymbolKind::Trigger => "trigger",
            SymbolKind::Index => "index",
        }
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => SymbolKind::Function,
            1 => SymbolKind::Struct,
            2 => SymbolKind::Enum,
            3 => SymbolKind::Trait,
            4 => SymbolKind::Impl,
            5 => SymbolKind::Class,
            6 => SymbolKind::Method,
            7 => SymbolKind::Type,
            8 => SymbolKind::Const,
            10 => SymbolKind::Table,
            11 => SymbolKind::View,
            12 => SymbolKind::Procedure,
            13 => SymbolKind::Trigger,
            14 => SymbolKind::Index,
            _ => SymbolKind::Other,
        }
    }
}

/// One symbol location — a named definition in a specific frame.
#[derive(Debug, Clone)]
pub struct SymbolEntry {
    /// Index into the SaidFile's trigram_doc_ids list (shared doc table).
    pub doc_index: u32,
    pub kind: SymbolKind,
    pub start_line: u32,
    pub end_line: u32,
}

/// Symbol table: name -> list of entries.
///
/// Same symbol name ("new", "serialize", "FrameStore") can live in many
/// places, so the value is a Vec. Lookup is O(log n) via BTreeMap.
pub struct SymbolIndex {
    /// Sorted by name. BTreeMap gives us prefix iteration for free.
    entries: BTreeMap<String, Vec<SymbolEntry>>,
}

impl SymbolIndex {
    pub fn new() -> Self {
        Self { entries: BTreeMap::new() }
    }

    /// Register a symbol definition.
    pub fn add(
        &mut self,
        name: &str,
        doc_index: u32,
        kind: SymbolKind,
        start_line: u32,
        end_line: u32,
    ) {
        self.entries.entry(name.to_string()).or_insert_with(Vec::new).push(
            SymbolEntry { doc_index, kind, start_line, end_line }
        );
    }

    /// Number of unique symbol names.
    pub fn num_names(&self) -> usize { self.entries.len() }

    /// Total number of symbol entries across all names.
    pub fn total_entries(&self) -> usize {
        self.entries.values().map(|v| v.len()).sum()
    }

    /// Iterate all entries — used by `symbols_snapshot` for copying to module brains.
    pub fn all_entries(&self) -> impl Iterator<Item = (&String, &Vec<SymbolEntry>)> {
        self.entries.iter()
    }

    /// Exact lookup — returns all entries for this name (possibly empty).
    pub fn lookup_exact(&self, name: &str) -> &[SymbolEntry] {
        self.entries.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Prefix lookup — returns (name, entries) pairs sorted alphabetically.
    /// Cap at `max_names` to avoid flooding the caller.
    pub fn lookup_prefix(&self, prefix: &str, max_names: usize) -> Vec<(&str, &[SymbolEntry])> {
        let mut out = Vec::new();
        for (name, entries) in self.entries.range(prefix.to_string()..) {
            if !name.starts_with(prefix) { break; }
            out.push((name.as_str(), entries.as_slice()));
            if out.len() >= max_names { break; }
        }
        out
    }

    /// Fuzzy lookup — case-insensitive contains-match. Slower but useful
    /// when the user doesn't remember the exact name.
    pub fn lookup_contains(&self, needle: &str, max_names: usize) -> Vec<(&str, &[SymbolEntry])> {
        let needle_lower = needle.to_lowercase();
        let mut out = Vec::new();
        for (name, entries) in &self.entries {
            if name.to_lowercase().contains(&needle_lower) {
                out.push((name.as_str(), entries.as_slice()));
                if out.len() >= max_names { break; }
            }
        }
        out
    }

    /// Iterate over all names (sorted). Used for debugging / stats.
    pub fn iter_names(&self) -> impl Iterator<Item = (&String, &Vec<SymbolEntry>)> {
        self.entries.iter()
    }

    // ─────────────────────────────────────────────────────────────────────
    // Serialization
    // ─────────────────────────────────────────────────────────────────────

    /// Serialize to a raw byte buffer. Layout:
    ///
    /// ```text
    /// u8        version (= 1)
    /// u8        reserved
    /// u16       reserved
    /// u32       num_names
    /// per name:
    ///   u16     name_len
    ///   bytes   name (utf8)
    ///   u16     n_entries
    ///   per entry:
    ///     u32   doc_index
    ///     u8    kind
    ///     u8    reserved
    ///     u32   start_line
    ///     u32   end_line
    /// ```
    pub fn serialize_raw(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.push(1u8); // version
        out.push(0u8); // reserved
        out.extend_from_slice(&0u16.to_le_bytes()); // reserved
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());

        for (name, entries) in &self.entries {
            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len().min(u16::MAX as usize) as u16;
            out.extend_from_slice(&name_len.to_le_bytes());
            out.extend_from_slice(&name_bytes[..name_len as usize]);

            let n_entries = entries.len().min(u16::MAX as usize) as u16;
            out.extend_from_slice(&n_entries.to_le_bytes());
            for e in entries.iter().take(n_entries as usize) {
                out.extend_from_slice(&e.doc_index.to_le_bytes());
                out.push(e.kind as u8);
                out.push(0u8); // reserved
                out.extend_from_slice(&e.start_line.to_le_bytes());
                out.extend_from_slice(&e.end_line.to_le_bytes());
            }
        }
        out
    }

    /// Deserialize from a raw buffer produced by `serialize_raw`.
    pub fn deserialize_raw(data: &[u8]) -> Result<Self, String> {
        if data.len() < 8 {
            return Err("SYMS: buffer too short".into());
        }
        let version = data[0];
        if version != 1 {
            return Err(format!("SYMS: unsupported version {}", version));
        }
        let num_names = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        let mut p = 8usize;
        let mut entries: BTreeMap<String, Vec<SymbolEntry>> = BTreeMap::new();

        for _ in 0..num_names {
            if p + 2 > data.len() { return Err("SYMS: truncated name_len".into()); }
            let name_len = u16::from_le_bytes(data[p..p+2].try_into().unwrap()) as usize;
            p += 2;
            if p + name_len > data.len() { return Err("SYMS: truncated name".into()); }
            let name = String::from_utf8_lossy(&data[p..p+name_len]).to_string();
            p += name_len;

            if p + 2 > data.len() { return Err("SYMS: truncated n_entries".into()); }
            let n_entries = u16::from_le_bytes(data[p..p+2].try_into().unwrap()) as usize;
            p += 2;

            let mut vec = Vec::with_capacity(n_entries);
            for _ in 0..n_entries {
                if p + 14 > data.len() { return Err("SYMS: truncated entry".into()); }
                let doc_index = u32::from_le_bytes(data[p..p+4].try_into().unwrap());
                let kind = SymbolKind::from_u8(data[p+4]);
                // data[p+5] = reserved
                let start_line = u32::from_le_bytes(data[p+6..p+10].try_into().unwrap());
                let end_line = u32::from_le_bytes(data[p+10..p+14].try_into().unwrap());
                vec.push(SymbolEntry { doc_index, kind, start_line, end_line });
                p += 14;
            }
            entries.insert(name, vec);
        }

        Ok(Self { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_lookup_exact() {
        let mut idx = SymbolIndex::new();
        idx.add("compact_block_dict", 42, SymbolKind::Function, 383, 543);
        idx.add("new", 10, SymbolKind::Function, 5, 8);
        idx.add("new", 11, SymbolKind::Function, 20, 25);

        let entries = idx.lookup_exact("compact_block_dict");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].doc_index, 42);
        assert_eq!(entries[0].kind, SymbolKind::Function);

        let news = idx.lookup_exact("new");
        assert_eq!(news.len(), 2);

        assert!(idx.lookup_exact("nonexistent").is_empty());
    }

    #[test]
    fn lookup_prefix_sorted() {
        let mut idx = SymbolIndex::new();
        idx.add("compact", 1, SymbolKind::Function, 1, 2);
        idx.add("compact_block_dict", 2, SymbolKind::Function, 3, 4);
        idx.add("compactify", 3, SymbolKind::Function, 5, 6);
        idx.add("different", 4, SymbolKind::Function, 7, 8);

        let hits = idx.lookup_prefix("compact", 10);
        let names: Vec<&str> = hits.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["compact", "compact_block_dict", "compactify"]);
    }

    #[test]
    fn lookup_contains_case_insensitive() {
        let mut idx = SymbolIndex::new();
        idx.add("FrameStore", 0, SymbolKind::Struct, 1, 100);
        idx.add("frame_store_new", 1, SymbolKind::Function, 5, 10);
        idx.add("unrelated", 2, SymbolKind::Function, 50, 60);

        let hits = idx.lookup_contains("frame", 10);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn serialize_roundtrip() {
        let mut idx = SymbolIndex::new();
        idx.add("compact_block_dict", 42, SymbolKind::Function, 383, 543);
        idx.add("CompressedBlock", 43, SymbolKind::Struct, 50, 80);
        idx.add("new", 10, SymbolKind::Function, 5, 8);
        idx.add("new", 11, SymbolKind::Method, 20, 25);

        let raw = idx.serialize_raw();
        let idx2 = SymbolIndex::deserialize_raw(&raw).unwrap();

        assert_eq!(idx.num_names(), idx2.num_names());
        assert_eq!(idx.total_entries(), idx2.total_entries());

        // Verify specific entries round-trip
        let news = idx2.lookup_exact("new");
        assert_eq!(news.len(), 2);
        assert_eq!(news[0].kind, SymbolKind::Function);
        assert_eq!(news[1].kind, SymbolKind::Method);
        assert_eq!(news[1].start_line, 20);
        assert_eq!(news[1].end_line, 25);

        let cb = idx2.lookup_exact("compact_block_dict");
        assert_eq!(cb.len(), 1);
        assert_eq!(cb[0].doc_index, 42);
        assert_eq!(cb[0].start_line, 383);

        let comp = idx2.lookup_exact("CompressedBlock");
        assert_eq!(comp[0].kind, SymbolKind::Struct);
    }

    #[test]
    fn kind_mapping() {
        assert_eq!(SymbolKind::from_ts_kind("function_item"), SymbolKind::Function);
        assert_eq!(SymbolKind::from_ts_kind("struct_item"), SymbolKind::Struct);
        assert_eq!(SymbolKind::from_ts_kind("class_definition"), SymbolKind::Class);
        assert_eq!(SymbolKind::from_ts_kind("nonsense_kind"), SymbolKind::Other);
    }
}
