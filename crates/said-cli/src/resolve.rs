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
fn read_default() -> Option<String> {
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
