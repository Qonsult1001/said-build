//! Lens file — a live view over a parent .said brain.
//!
//! A lens file (e.g., `card.vivere.said`) is NOT a copy.
//! It references a parent brain and stores:
//!   - parent path (vivere.said)
//!   - module name (card)
//!   - filter tag (module:card)
//!   - local additions (developer's own frames — notes, new code)
//!
//! When queried, it reads from the parent brain filtered to module frames,
//! plus any local additions. Always fresh — no manual sync needed.

use std::path::{Path, PathBuf};
use std::collections::HashSet;

/// Magic bytes to identify a lens file vs a regular .said file
const LENS_MAGIC: &[u8; 4] = b"LENS";

/// A lens file — metadata-only, reads from parent brain.
pub struct LensFile {
    /// Path to this lens file
    pub path: PathBuf,
    /// Path to the parent .said brain (relative or absolute)
    pub parent_path: String,
    /// Module name (e.g., "card")
    pub module_name: String,
    /// Tag used to filter frames: "module:card"
    pub filter_tag: String,
    /// Doc IDs that belong to this module (cached from last snapshot)
    pub frame_ids: HashSet<String>,
    /// Timestamp of last snapshot/refresh
    pub snapshot_time: u64,
}

impl LensFile {
    /// Create a new lens file pointing to a parent brain.
    pub fn create(
        path: impl AsRef<Path>,
        parent_path: &str,
        module_name: &str,
        frame_ids: HashSet<String>,
    ) -> Self {
        let now = crate::time_compat::unix_secs();

        Self {
            path: path.as_ref().to_path_buf(),
            parent_path: parent_path.to_string(),
            module_name: module_name.to_string(),
            filter_tag: format!("module:{}", module_name.to_lowercase()),
            frame_ids,
            snapshot_time: now,
        }
    }

    /// Save the lens file to disk.
    pub fn save(&self) -> Result<(), String> {
        let mut data = Vec::new();

        // Magic
        data.extend_from_slice(LENS_MAGIC);

        // Parent path (length-prefixed string)
        let parent_bytes = self.parent_path.as_bytes();
        data.extend_from_slice(&(parent_bytes.len() as u32).to_le_bytes());
        data.extend_from_slice(parent_bytes);

        // Module name
        let module_bytes = self.module_name.as_bytes();
        data.extend_from_slice(&(module_bytes.len() as u32).to_le_bytes());
        data.extend_from_slice(module_bytes);

        // Snapshot time
        data.extend_from_slice(&self.snapshot_time.to_le_bytes());

        // Frame IDs count + data
        data.extend_from_slice(&(self.frame_ids.len() as u32).to_le_bytes());
        for id in &self.frame_ids {
            let id_bytes = id.as_bytes();
            data.extend_from_slice(&(id_bytes.len() as u16).to_le_bytes());
            data.extend_from_slice(id_bytes);
        }

        std::fs::write(&self.path, &data)
            .map_err(|e| format!("write lens: {}", e))
    }

    /// Open an existing lens file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let data = std::fs::read(&path)
            .map_err(|e| format!("read lens: {}", e))?;

        if data.len() < 4 || &data[0..4] != LENS_MAGIC {
            return Err("Not a lens file".to_string());
        }

        let mut pos = 4;

        // Parent path
        let parent_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        let parent_path = String::from_utf8_lossy(&data[pos..pos+parent_len]).to_string();
        pos += parent_len;

        // Module name
        let module_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        let module_name = String::from_utf8_lossy(&data[pos..pos+module_len]).to_string();
        pos += module_len;

        // Snapshot time
        let snapshot_time = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
        pos += 8;

        // Frame IDs
        let frame_count = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        let mut frame_ids = HashSet::new();
        for _ in 0..frame_count {
            if pos + 2 > data.len() { break; }
            let id_len = u16::from_le_bytes(data[pos..pos+2].try_into().unwrap()) as usize;
            pos += 2;
            if pos + id_len > data.len() { break; }
            let id = String::from_utf8_lossy(&data[pos..pos+id_len]).to_string();
            pos += id_len;
            frame_ids.insert(id);
        }

        let filter_tag = format!("module:{}", module_name.to_lowercase());
        Ok(Self {
            path,
            parent_path,
            module_name,
            filter_tag,
            frame_ids,
            snapshot_time,
        })
    }

    /// Check if a file is a lens file (by reading the magic bytes).
    pub fn is_lens(path: impl AsRef<Path>) -> bool {
        if let Ok(data) = std::fs::read(path.as_ref()) {
            data.len() >= 4 && &data[0..4] == LENS_MAGIC
        } else {
            false
        }
    }

    /// Resolve the parent path relative to the lens file's directory.
    pub fn resolve_parent(&self) -> PathBuf {
        let parent = Path::new(&self.parent_path);
        if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            // Relative to lens file directory
            self.path.parent()
                .unwrap_or(Path::new("."))
                .join(parent)
        }
    }

    /// Check if a doc_id belongs to this module.
    pub fn contains(&self, doc_id: &str) -> bool {
        self.frame_ids.contains(doc_id)
    }

    /// Add new frame IDs (when parent brain has new card-related frames).
    pub fn add_frames(&mut self, new_ids: impl IntoIterator<Item = String>) {
        self.frame_ids.extend(new_ids);
    }

    /// Update snapshot timestamp.
    pub fn touch(&mut self) {
        self.snapshot_time = crate::time_compat::unix_secs();
    }
}
