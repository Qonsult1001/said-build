//! Walk a forge workspace and classify files by folder + content.
//!
//! Used by `forge plan` to build scan-driven Q&A options and by `forge sync`
//! to enumerate what needs ingesting.

use std::path::{Path, PathBuf};

use crate::{ForgeError, ForgeResult};

/// Snapshot of a workspace scan.
#[derive(Debug, Clone)]
pub struct ProjectScan {
    pub root: PathBuf,
    pub ground_truth: Vec<ScannedFile>,
    pub progress: Vec<ScannedFile>,
    pub requirements: Vec<ScannedFile>,
    pub expectations: Vec<ScannedFile>,
    /// At least one file in `4-expectations/` matched the OpenAPI sniff test.
    pub has_openapi: bool,
    /// At least one `.xlsx`/`.xlsm` file found (in any folder; usually requirements).
    pub has_xlsx: bool,
    /// At least one Markdown file in `4-expectations/` (likely Dev Planning).
    pub has_dev_planning_md: bool,
    /// Files sitting at the project root that aren't in any of the four
    /// authority folders. Warning only — sync ignores them.
    pub orphan_files: Vec<ScannedFile>,
}

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub extension: String,
    pub size_bytes: u64,
}

/// Top-level scan. Safe to call on a partially-populated workspace; missing
/// folders are silently ignored.
pub fn scan_project(root: &Path) -> ForgeResult<ProjectScan> {
    let mut scan = ProjectScan {
        root: root.to_path_buf(),
        ground_truth: Vec::new(),
        progress: Vec::new(),
        requirements: Vec::new(),
        expectations: Vec::new(),
        has_openapi: false,
        has_xlsx: false,
        has_dev_planning_md: false,
        orphan_files: Vec::new(),
    };

    for folder_name in crate::init::FOLDER_NAMES {
        let dir = root.join(folder_name);
        if !dir.is_dir() {
            continue;
        }
        let files = walk_files(&dir)?;
        for f in files {
            let ext = f.extension.clone();
            match folder_name {
                "4-expectations" => {
                    if (ext == "yaml" || ext == "yml") && quick_is_openapi(&f.path) {
                        scan.has_openapi = true;
                    }
                    if ext == "md" || ext == "markdown" {
                        scan.has_dev_planning_md = true;
                    }
                    scan.expectations.push(f);
                }
                "3-requirements" => {
                    if ext == "xlsx" || ext == "xlsm" {
                        scan.has_xlsx = true;
                    }
                    scan.requirements.push(f);
                }
                "2-progress" => scan.progress.push(f),
                "1-ground-truth" => scan.ground_truth.push(f),
                _ => {}
            }
        }
    }

    // Orphans at root — files not in one of the four folders. Skip known
    // scaffold artefacts (.said, README.md, .forge) and hidden files.
    if root.is_dir() {
        for entry in std::fs::read_dir(root).map_err(|e| io_err(root, e))? {
            let entry = entry.map_err(|e| io_err(root, e))?;
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.starts_with('.')
                || name.eq_ignore_ascii_case("README.md")
                || name.ends_with(".said")
            {
                continue;
            }
            let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            scan.orphan_files.push(ScannedFile {
                extension: p.extension().and_then(|e| e.to_str()).unwrap_or("").to_string(),
                size_bytes,
                path: p,
            });
        }
    }

    Ok(scan)
}

/// Shallow sniff — reads the first ~1 KB of a YAML file and looks for the
/// `openapi:` root key.
pub fn quick_is_openapi(p: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(p) else {
        return false;
    };
    let head: String = content.chars().take(1024).collect();
    head.lines().any(|line| {
        let t = line.trim_start();
        t.starts_with("openapi:") || t.starts_with("openapi :")
    })
}

fn walk_files(dir: &Path) -> ForgeResult<Vec<ScannedFile>> {
    let mut out = Vec::new();
    walk_recursive(dir, &mut out)?;
    Ok(out)
}

fn walk_recursive(dir: &Path, out: &mut Vec<ScannedFile>) -> ForgeResult<()> {
    for entry in std::fs::read_dir(dir).map_err(|e| io_err(dir, e))? {
        let entry = entry.map_err(|e| io_err(dir, e))?;
        let p = entry.path();
        if p.is_dir() {
            walk_recursive(&p, out)?;
        } else if p.is_file() {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name == "README.md" || name == ".gitkeep" || name.starts_with('.') {
                continue;
            }
            // Skip adopter intermediate files. The proc-framework adopter writes
            // `.adopted.sql` (output) and `.pre-adoption.sql` (backup of the
            // original) into the same directory as the live `.sql`. If those
            // get synced into the brain, they ingest as additional frames and
            // every one becomes a separate CREATE PROCEDURE in the generated
            // sandbox schema.sql — the first CREATE wins (often the stale one),
            // and edits to the live `.sql` silently get shadowed. Treat these
            // as intermediates that never enter the source-of-truth set.
            if name.ends_with(".adopted.sql")
                || name.ends_with(".pre-adoption.sql")
                || name.ends_with(".pre-adoption.adopted.sql")
            {
                continue;
            }
            let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(ScannedFile {
                extension: p
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase(),
                size_bytes,
                path: p,
            });
        }
    }
    Ok(())
}

fn io_err(p: &Path, e: std::io::Error) -> ForgeError {
    ForgeError::Io {
        path: p.display().to_string(),
        cause: e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::{apply_scaffold, ApplyMode};

    fn scaffolded_workspace() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        apply_scaffold("w", &root, ApplyMode::Merge).unwrap();
        (tmp, root)
    }

    #[test]
    fn scan_empty_workspace_returns_zero_counts() {
        let (_tmp, root) = scaffolded_workspace();
        let scan = scan_project(&root).unwrap();
        assert!(scan.ground_truth.is_empty());
        assert!(scan.progress.is_empty());
        assert!(scan.requirements.is_empty());
        assert!(scan.expectations.is_empty());
        assert!(!scan.has_openapi);
        assert!(!scan.has_xlsx);
        assert!(!scan.has_dev_planning_md);
    }

    #[test]
    fn scan_classifies_files_into_correct_folders() {
        let (_tmp, root) = scaffolded_workspace();
        std::fs::write(root.join("1-ground-truth/schema.sql"), "CREATE TABLE t();").unwrap();
        std::fs::write(root.join("2-progress/Service.cs"), "// existing c# code").unwrap();
        std::fs::write(root.join("3-requirements/BIN.pdf"), "%PDF-fake").unwrap();
        std::fs::write(root.join("3-requirements/reqs.xlsm"), "binary").unwrap();
        std::fs::write(
            root.join("4-expectations/api.yaml"),
            "openapi: 3.0.0\ninfo:\n  title: X\n",
        )
        .unwrap();
        std::fs::write(root.join("4-expectations/OVERVIEW.md"), "# overview").unwrap();

        let scan = scan_project(&root).unwrap();
        assert_eq!(scan.ground_truth.len(), 1);
        assert_eq!(scan.progress.len(), 1);
        assert_eq!(scan.requirements.len(), 2);
        assert_eq!(scan.expectations.len(), 2);
        assert!(scan.has_openapi);
        assert!(scan.has_xlsx);
        assert!(scan.has_dev_planning_md);
    }

    #[test]
    fn scan_skips_readme_and_gitkeep_and_dotfiles() {
        let (_tmp, root) = scaffolded_workspace();
        // The scaffold already wrote README.md + .gitkeep in every folder —
        // scan should ignore them.
        let scan = scan_project(&root).unwrap();
        assert!(scan.ground_truth.is_empty());
    }

    #[test]
    fn scan_recurses_into_subfolders() {
        let (_tmp, root) = scaffolded_workspace();
        std::fs::create_dir_all(root.join("4-expectations/Dev Planning")).unwrap();
        std::fs::write(root.join("4-expectations/Dev Planning/API.md"), "# api").unwrap();
        let scan = scan_project(&root).unwrap();
        assert_eq!(scan.expectations.len(), 1);
        assert!(scan.has_dev_planning_md);
    }

    #[test]
    fn scan_detects_openapi_by_content_not_filename() {
        let (_tmp, root) = scaffolded_workspace();
        std::fs::write(
            root.join("4-expectations/not-openapi.yaml"),
            "name: foo\nvalue: 42\n",
        )
        .unwrap();
        let s1 = scan_project(&root).unwrap();
        assert!(!s1.has_openapi);

        std::fs::write(
            root.join("4-expectations/real-openapi.yaml"),
            "openapi: 3.0.3\ninfo: {}\n",
        )
        .unwrap();
        let s2 = scan_project(&root).unwrap();
        assert!(s2.has_openapi);
    }

    #[test]
    fn scan_flags_orphan_files_at_root() {
        let (_tmp, root) = scaffolded_workspace();
        std::fs::write(root.join("orphan.txt"), "stray file").unwrap();
        let scan = scan_project(&root).unwrap();
        assert_eq!(scan.orphan_files.len(), 1);
        assert!(scan.orphan_files[0].path.ends_with("orphan.txt"));
    }

    #[test]
    fn scan_extensions_are_lowercased() {
        let (_tmp, root) = scaffolded_workspace();
        std::fs::write(root.join("3-requirements/MIXED.PDF"), "%PDF").unwrap();
        let scan = scan_project(&root).unwrap();
        assert_eq!(scan.requirements[0].extension, "pdf");
    }
}
