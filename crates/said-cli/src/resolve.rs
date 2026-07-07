//! Auto-detect which .said file to use.
//!
//! Priority:
//! 1. Explicit --path flag
//! 2. Single *.said in current directory
//! 3. Default file from config (APPDATA/said/default on Windows, ~/.config/said/default on Unix)
//! 4. Error with helpful message

use std::path::{Path, PathBuf};

/// Resolve the .said file path.
pub fn resolve(explicit: Option<&str>) -> Result<PathBuf, String> {
    // 1. Explicit path
    if let Some(p) = explicit {
        return Ok(PathBuf::from(p));
    }

    // 2. Single .said file in current directory
    if let Ok(entries) = std::fs::read_dir(".") {
        let said_files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "said").unwrap_or(false))
            .collect();
        if said_files.len() == 1 {
            return Ok(said_files[0].clone());
        }
    }

    // 3. Default from config
    if let Some(default_path) = read_default() {
        if Path::new(&default_path).exists() {
            return Ok(PathBuf::from(default_path));
        }
    }

    Err(
        "No .said file found. Options:\n  \
         1. said create my_brain.said\n  \
         2. said --path my_brain.said <command>\n  \
         3. said use my_brain.said  (sets default)"
            .into(),
    )
}

/// Config directory for said defaults.
fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(|d| PathBuf::from(d).join("said"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        dirs_next::config_dir().map(|d| d.join("said"))
    }
}

/// Read the default .said file path from config.
pub fn read_default() -> Option<String> {
    let dir = config_dir()?;
    let file = dir.join("default");
    std::fs::read_to_string(file).ok().map(|s| s.trim().to_string())
}

/// Set the default .said file path.
pub fn set_default(path: &str) -> Result<(), String> {
    let dir = config_dir().ok_or("Cannot determine config directory")?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create config dir: {}", e))?;
    let abs = std::fs::canonicalize(path)
        .map_err(|e| format!("Cannot resolve path '{}': {}", path, e))?;
    std::fs::write(dir.join("default"), abs.to_string_lossy().as_bytes())
        .map_err(|e| format!("Failed to write default: {}", e))
}

// ── Key/value config (persisted to config.json in the config dir) ──────────────
// `said config <key> <value>` sets, `said config <key>` gets. Previously a no-op stub
// (set printed "Set" but stored nothing; get always said "not set") — now persisted.

fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.json"))
}

fn read_config_map() -> std::collections::BTreeMap<String, String> {
    config_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Get a config value by key (None if unset).
pub fn get_config(key: &str) -> Option<String> {
    read_config_map().get(key).cloned()
}

/// All config key/value pairs (sorted).
pub fn list_config() -> std::collections::BTreeMap<String, String> {
    read_config_map()
}

/// Set (or overwrite) a config key. Persists the whole map to config.json.
pub fn set_config(key: &str, value: &str) -> Result<(), String> {
    let dir = config_dir().ok_or("Cannot determine config directory")?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create config dir: {}", e))?;
    let mut map = read_config_map();
    map.insert(key.to_string(), value.to_string());
    let json = serde_json::to_string_pretty(&map)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(dir.join("config.json"), json)
        .map_err(|e| format!("Failed to write config: {}", e))
}
