//! Workspace scaffolding for `said forge init`.
//!
//! Creates the four authority folders, per-folder README files, a `.forge/`
//! metadata directory, a top-level README, and an empty `.said` brain. The
//! scaffold is what users drop their source content into before running
//! `forge plan` and `forge sync`.

use sca_core::said_file::{BrainMode, SaidFile};
use std::fs;
use std::path::{Path, PathBuf};

use crate::{ForgeError, ForgeResult};

/// The four authority folders, in human-readable ingest order.
pub const FOLDER_NAMES: [&str; 4] = [
    "1-ground-truth",
    "2-progress",
    "3-requirements",
    "4-expectations",
];

/// How aggressively `apply_scaffold` should handle a target directory that
/// already contains files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyMode {
    /// Error if the target directory is non-empty. Default — prevents
    /// accidental overwrite of user data.
    Strict,
    /// Create missing files and folders, skip existing files. Idempotent.
    Merge,
    /// Overwrite existing README/config files. Never destroys `.said`.
    Force,
}

/// A resolved scaffold — what will be written to disk.
pub struct ScaffoldPlan {
    pub project_name: String,
    pub folders: Vec<FolderEntry>,
    pub files: Vec<FileEntry>,
}

pub struct FolderEntry {
    pub path: String,
    pub readme: String,
}

pub struct FileEntry {
    pub path: String,
    pub contents: String,
}

/// Outcome of `apply_scaffold`. Counts are informational; errors propagate.
#[derive(Debug, Clone)]
pub struct ApplyResult {
    pub folders_created: usize,
    pub files_created: usize,
    pub files_skipped: usize,
    pub said_path: PathBuf,
}

/// Build the scaffold in memory — pure function, no I/O.
pub fn scaffold_plan(project_name: &str) -> ScaffoldPlan {
    let folders = vec![
        FolderEntry {
            path: "1-ground-truth".into(),
            readme: ground_truth_readme(),
        },
        FolderEntry {
            path: "2-progress".into(),
            readme: progress_readme(),
        },
        FolderEntry {
            path: "3-requirements".into(),
            readme: requirements_readme(),
        },
        FolderEntry {
            path: "4-expectations".into(),
            readme: expectations_readme(),
        },
    ];
    let files = vec![
        FileEntry {
            path: format!("{}.said", project_name),
            contents: String::new(),
        },
        FileEntry {
            path: ".forge/config.toml".into(),
            contents: config_stub(project_name),
        },
        FileEntry {
            path: "README.md".into(),
            contents: top_readme(project_name),
        },
    ];
    ScaffoldPlan {
        project_name: project_name.into(),
        folders,
        files,
    }
}

/// Execute the scaffold — creates folders, writes files, creates empty `.said`.
pub fn apply_scaffold(
    project_name: &str,
    root: &Path,
    mode: ApplyMode,
) -> ForgeResult<ApplyResult> {
    if root.exists() && mode == ApplyMode::Strict && has_meaningful_contents(root)? {
        return Err(ForgeError::Validation(format!(
            "target directory is non-empty and mode=Strict: {}. \
             Use --merge to create missing files only, or --force to overwrite.",
            root.display()
        )));
    }
    fs::create_dir_all(root).map_err(|e| io_err(root, e))?;
    let plan = scaffold_plan(project_name);
    let mut folders_created = 0usize;
    let mut files_created = 0usize;
    let mut files_skipped = 0usize;

    for f in &plan.folders {
        let folder_path = root.join(&f.path);
        let existed = folder_path.exists();
        fs::create_dir_all(&folder_path).map_err(|e| io_err(&folder_path, e))?;
        if !existed {
            folders_created += 1;
        }
        let readme_path = folder_path.join("README.md");
        write_file_with_mode(&readme_path, &f.readme, mode, &mut files_created, &mut files_skipped)?;
        let gitkeep_path = folder_path.join(".gitkeep");
        write_file_with_mode(&gitkeep_path, "", mode, &mut files_created, &mut files_skipped)?;
    }

    let forge_dir = root.join(".forge");
    let forge_dir_existed = forge_dir.exists();
    fs::create_dir_all(&forge_dir).map_err(|e| io_err(&forge_dir, e))?;
    if !forge_dir_existed {
        folders_created += 1;
    }

    for file in &plan.files {
        if file.path.ends_with(".said") {
            continue;
        }
        let path = root.join(&file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
        }
        write_file_with_mode(&path, &file.contents, mode, &mut files_created, &mut files_skipped)?;
    }

    let said_path = root.join(format!("{}.said", project_name));
    if !said_path.exists() {
        let mut sf = SaidFile::create_with_mode(&said_path, BrainMode::Portable);
        sf.save()
            .map_err(|e| ForgeError::Validation(format!("SaidFile save failed: {}", e)))?;
        files_created += 1;
    } else if mode == ApplyMode::Force {
        // Never clobber a .said in Force mode either — too destructive.
        files_skipped += 1;
    } else {
        files_skipped += 1;
    }

    Ok(ApplyResult {
        folders_created,
        files_created,
        files_skipped,
        said_path,
    })
}

fn write_file_with_mode(
    path: &Path,
    contents: &str,
    mode: ApplyMode,
    created: &mut usize,
    skipped: &mut usize,
) -> ForgeResult<()> {
    if path.exists() {
        match mode {
            ApplyMode::Strict => {
                return Err(ForgeError::Validation(format!(
                    "file exists and mode=Strict: {}",
                    path.display()
                )));
            }
            ApplyMode::Merge => {
                *skipped += 1;
                return Ok(());
            }
            ApplyMode::Force => {}
        }
    }
    fs::write(path, contents).map_err(|e| io_err(path, e))?;
    *created += 1;
    Ok(())
}

fn has_meaningful_contents(dir: &Path) -> ForgeResult<bool> {
    let entries = fs::read_dir(dir).map_err(|e| io_err(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| io_err(dir, e))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == ".git" || name_str == ".DS_Store" || name_str == "Thumbs.db" {
            continue;
        }
        return Ok(true);
    }
    Ok(false)
}

fn io_err(path: &Path, cause: std::io::Error) -> ForgeError {
    ForgeError::Io {
        path: path.display().to_string(),
        cause,
    }
}

fn ground_truth_readme() -> String {
    "# 1-ground-truth\n\n\
     **Authority: law.**\n\n\
     Put **authoritative source code** here — usually SQL schema and stored procedures. \
     If ground truth disagrees with anything else (existing code, client wishlist), \
     ground truth wins.\n\n\
     Forge will ingest `.sql`, `.rs`, `.py`, `.go`, `.ts`, `.cs`, `.java` via code ingest \
     into the Code pillar with tag `authority:law:ground-truth`.\n"
        .into()
}

fn progress_readme() -> String {
    "# 2-progress\n\n\
     **Authority: existing (reference or authoritative, configurable).**\n\n\
     Put **existing implementation work** here: current framework code, prior skills files, \
     worked-out example user-stories from earlier iterations. Forge treats these as \
     reference-grade by default — useful for grounding but not authoritative unless you \
     opt in during `forge plan`.\n\n\
     Ingests code files as Code pillar + docs/examples as External pillar, \
     tag `authority:existing:progress`.\n"
        .into()
}

fn requirements_readme() -> String {
    "# 3-requirements\n\n\
     **Authority: requested (hard by default).**\n\n\
     Put **raw client-stated requirements** here: PDFs, `.docx`, `.xlsx`/`.xlsm` requirements \
     catalogues, plain-text notes. These are things the client told you they need.\n\n\
     Ingests to External pillar with tag `authority:requested:requirements`. \
     Per-file overrides available during `forge plan` (some PDFs may be background/soft \
     rather than hard requirements).\n"
        .into()
}

fn expectations_readme() -> String {
    "# 4-expectations\n\n\
     **Authority: depends — answered during `forge plan`.**\n\n\
     Put **the contract** here: OpenAPI spec + Dev Planning MDs. One of these will be chosen \
     as the forge directive (the file forge iterates to generate stories). The other becomes \
     grounding context.\n\n\
     - `.yaml`/`.yml` with `openapi:` key → typically the client wishlist\n\
     - `.md` Dev Planning docs → typically the agreed scope\n\n\
     During `forge plan` you'll pick which is authoritative on conflict.\n"
        .into()
}

fn config_stub(project_name: &str) -> String {
    format!(
        "# Forge config for {}\n\
         # Written by `forge plan`. Until plan runs, the scaffold is present but no \
         generation is possible.\n\
         version = 1\n\
         plan_complete = false\n",
        project_name
    )
}

fn top_readme(project_name: &str) -> String {
    format!(
        "# {name} — forge workspace\n\n\
         This project is organised for `said forge`. Four authority-graded folders feed into \
         `{name}.said`:\n\n\
         1. `1-ground-truth/` — SQL + authoritative code. Law.\n\
         2. `2-progress/` — existing framework code + prior skills/examples.\n\
         3. `3-requirements/` — client PDFs + XLSM requirements.\n\
         4. `4-expectations/` — OpenAPI + Dev Planning MDs. One is the directive.\n\n\
         ## Workflow\n\n\
         ```bash\n\
         said forge init {name}        # you already ran this\n\
         # → drop files into the four folders\n\
         said forge plan               # interactive Q&A — resolves authorities\n\
         said forge sync               # ingest everything into {name}.said\n\
         said forge run                # generate stories + gap report\n\
         ```\n\n\
         See each folder's `README.md` for what belongs where.\n",
        name = project_name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_plan_lists_four_folders_and_files() {
        let plan = scaffold_plan("dt");
        let folders: Vec<&str> = plan.folders.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            folders,
            vec!["1-ground-truth", "2-progress", "3-requirements", "4-expectations"]
        );
        assert!(plan.files.iter().any(|f| f.path == "dt.said"));
        assert!(plan.files.iter().any(|f| f.path == ".forge/config.toml"));
        assert!(plan.files.iter().any(|f| f.path == "README.md"));
    }

    #[test]
    fn folder_readmes_mention_authority_level() {
        let plan = scaffold_plan("dt");
        assert!(plan.folders[0].readme.contains("law"));
        assert!(plan.folders[1].readme.contains("existing"));
        assert!(plan.folders[2].readme.contains("requested"));
        assert!(plan.folders[3].readme.contains("contract"));
    }

    #[test]
    fn config_stub_has_version_and_plan_incomplete() {
        let plan = scaffold_plan("dt");
        let cfg = &plan
            .files
            .iter()
            .find(|f| f.path == ".forge/config.toml")
            .unwrap()
            .contents;
        assert!(cfg.contains("version = 1"));
        assert!(cfg.contains("plan_complete = false"));
    }

    #[test]
    fn apply_scaffold_creates_folders_files_and_said() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("dt");
        let result = apply_scaffold("dt", &root, ApplyMode::Strict).unwrap();

        for folder in FOLDER_NAMES {
            assert!(root.join(folder).is_dir(), "missing folder: {}", folder);
            assert!(root.join(folder).join("README.md").is_file());
            assert!(root.join(folder).join(".gitkeep").is_file());
        }
        assert!(root.join(".forge").is_dir());
        assert!(root.join(".forge/config.toml").is_file());
        assert!(root.join("README.md").is_file());
        assert!(root.join("dt.said").is_file());
        assert_eq!(result.folders_created, 5); // 4 authority + .forge
        // 4 READMEs + 4 gitkeeps + 1 top README + 1 config + 1 said = 11
        assert!(result.files_created >= 10, "got {}", result.files_created);
    }

    #[test]
    fn apply_scaffold_twice_with_merge_is_idempotent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("dt");
        let first = apply_scaffold("dt", &root, ApplyMode::Strict).unwrap();
        assert!(first.files_created > 0);

        let second = apply_scaffold("dt", &root, ApplyMode::Merge).unwrap();
        assert_eq!(second.files_created, 0);
        assert!(second.files_skipped >= 10);
        assert!(root.join("dt.said").is_file());
    }

    #[test]
    fn apply_scaffold_twice_with_strict_fails() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("dt");
        apply_scaffold("dt", &root, ApplyMode::Strict).unwrap();
        let err = apply_scaffold("dt", &root, ApplyMode::Strict).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("non-empty") || msg.contains("exists"), "got: {}", msg);
    }

    #[test]
    fn apply_scaffold_ignores_git_dir_in_strict_mode() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("dt");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
        // Strict should succeed because .git is an allow-listed pre-existing entry.
        let result = apply_scaffold("dt", &root, ApplyMode::Strict).unwrap();
        assert!(result.folders_created > 0);
        assert!(root.join("dt.said").is_file());
    }
}
