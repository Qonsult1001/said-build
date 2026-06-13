//! XLSX / XLSM ingester via calamine (pure Rust).
//!
//! Two modes — controlled by `WorkspaceConfig::xlsx.mode`:
//! - `GroundingOnly`: each non-empty row becomes a searchable External-pillar
//!   frame with JSON-object content. Useful for requirements catalogues that
//!   forge should be able to cite but not iterate.
//! - `StoryPerRow`: same ingest + each row also becomes a `StoryKind::TableRow`
//!   story so `forge run` generates spec/plan/tasks per-row. Slug derived from
//!   an id-like column (id, key, code, ref, req_id, story_id, ticket).
//!
//! Sheets matching any substring in `xlsx.sheet_skip_patterns` (default:
//! `_drafts`, `_backup`, `~temp`) are skipped. Empty rows skipped. Dates
//! serialise to ISO-8601 strings when calamine provides them; otherwise fall
//! back to raw f64 serial day.

#![cfg(feature = "forge-xlsx")]

use calamine::{open_workbook_auto, Data, Reader, Sheets};
use std::collections::BTreeMap;
use std::path::Path;

use crate::{ForgeError, ForgeResult};

use super::macro_strip;
use super::xlsx_styles::{self, CellColor, WorkbookStyles};

/// Reads an `.xlsx`/`.xlsm` file. Macros are ignored — we only access cell
/// data, not VBA.
pub struct XlsxReader {
    workbook: Sheets<std::io::BufReader<std::fs::File>>,
    /// Raw zip bytes — kept for parsing styles.xml and sheet XML for colors.
    zip_bytes: Vec<u8>,
    /// Styles parsed on first color lookup. Cached so repeated reads don't
    /// re-parse styles.xml for every sheet.
    styles: Option<Option<WorkbookStyles>>,
}

impl XlsxReader {
    pub fn open(path: &Path) -> ForgeResult<Self> {
        let bytes = std::fs::read(path).map_err(|e| ForgeError::Io {
            path: path.display().to_string(),
            cause: e,
        })?;
        // Try directly. Calamine handles vanilla `.xlsx` fine.
        match open_workbook_auto(path) {
            Ok(wb) => Ok(Self {
                workbook: wb,
                zip_bytes: bytes,
                styles: None,
            }),
            Err(e) => {
                let raw = format!("{}", e);
                if raw.contains("macrosheets") || raw.contains("sheet:type") {
                    // `.xlsm` with Excel 4.0 macro sheets — calamine refuses.
                    // Strip macrosheets to a temp file, retry.
                    let cleaned = macro_strip::strip_macrosheets_to_temp(path)?;
                    let cleaned_bytes = std::fs::read(&cleaned).map_err(|e| ForgeError::Io {
                        path: cleaned.display().to_string(),
                        cause: e,
                    })?;
                    let wb =
                        open_workbook_auto(&cleaned).map_err(|e2| ForgeError::Parse {
                            path: path.display().to_string(),
                            message: format!("calamine open (after macro strip): {}", e2),
                        })?;
                    Ok(Self {
                        workbook: wb,
                        zip_bytes: cleaned_bytes,
                        styles: None,
                    })
                } else {
                    Err(ForgeError::Parse {
                        path: path.display().to_string(),
                        message: format!("calamine open: {}", raw),
                    })
                }
            }
        }
    }

    pub fn sheet_names(&self) -> Vec<String> {
        self.workbook.sheet_names()
    }

    /// Read a sheet with all enrichments: smart header detection, merged-cell
    /// unfolding, and row-color extraction.
    pub fn read_sheet(&mut self, sheet_name: &str) -> ForgeResult<SheetData> {
        let range = self
            .workbook
            .worksheet_range(sheet_name)
            .map_err(|e| ForgeError::Parse {
                path: sheet_name.into(),
                message: format!("worksheet_range: {}", e),
            })?;

        // Step 1: materialise the raw grid. Each cell → String via cell_to_string.
        let mut grid: Vec<Vec<String>> = range
            .rows()
            .map(|r| r.iter().map(cell_to_string).collect::<Vec<String>>())
            .collect();
        if grid.is_empty() {
            return Ok(SheetData::empty(sheet_name));
        }

        // Step 2: unfold merged cells. calamine returns the value only in the
        // top-left cell of a merge; copy it into the rest so each row has
        // complete data for the columns it covers.
        // Fetch merged-cell ranges by parsing sheet XML ourselves. `Sheets<RS>`
        // (calamine's polymorphic wrapper) doesn't expose worksheet_merge_cells
        // — that method lives on the concrete `Xlsx<RS>` inside. Pulling merges
        // directly from sheet XML keeps the dispatch uniform.
        let sheet_xml_path = self.sheet_xml_path(sheet_name);
        let merges: Vec<((u32, u32), (u32, u32))> = match &sheet_xml_path {
            Some(p) => super::xlsx_styles::extract_merged_ranges(&self.zip_bytes, p)
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let (start_row, start_col) = range.start().unwrap_or((0, 0));
        for (start, end) in &merges {
            let sr = (start.0).saturating_sub(start_row) as usize;
            let sc = (start.1).saturating_sub(start_col) as usize;
            let er = (end.0).saturating_sub(start_row) as usize;
            let ec = (end.1).saturating_sub(start_col) as usize;
            let Some(top_row) = grid.get(sr) else { continue };
            if sc >= top_row.len() {
                continue;
            }
            let val = grid[sr][sc].clone();
            let max_r = er.min(grid.len().saturating_sub(1));
            for r in sr..=max_r {
                let row_len = grid[r].len();
                let max_c = ec.min(row_len.saturating_sub(1));
                for c in sc..=max_c {
                    if (r, c) != (sr, sc) && grid[r][c].is_empty() {
                        grid[r][c] = val.clone();
                    }
                }
            }
        }

        // Step 3: smart header detection. Scan the first 10 rows for the row
        // that looks most like a header: many non-empty cells, mostly short
        // strings, mostly unique. Fall back to row 0 if no clear winner.
        let header_row_idx = detect_header_row(&grid);

        // Step 4: split into headers + data rows.
        let headers: Vec<String> = grid
            .get(header_row_idx)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, h)| {
                if h.trim().is_empty() {
                    format!("col_{}", i + 1)
                } else {
                    h
                }
            })
            .collect();

        let mut rows = Vec::new();
        let mut row_numbers = Vec::new();
        for (i, row) in grid.iter().enumerate().skip(header_row_idx + 1) {
            if row.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            rows.push(row.clone());
            // Excel row number is 1-indexed and includes every row (including
            // ones we skipped above the header). `start_row` is the grid's
            // Excel origin.
            row_numbers.push((start_row as u32) + 1 + (i as u32));
        }

        // Step 5: color enrichment — look up row colors from styles.xml.
        let row_colors = self.row_colors_for(sheet_name)?;

        Ok(SheetData {
            name: sheet_name.to_string(),
            headers,
            rows,
            row_numbers,
            header_row_number: (start_row as u32) + 1 + (header_row_idx as u32),
            row_colors,
        })
    }

    /// Resolve the real sheet-XML path via workbook.xml + workbook.xml.rels.
    /// Sheet names don't map 1:1 to `sheet{N}.xml` filenames in the zip —
    /// relationships do.
    fn sheet_xml_path(&mut self, sheet_name: &str) -> Option<String> {
        super::xlsx_styles::resolve_sheet_path(&self.zip_bytes, sheet_name)
    }

    fn row_colors_for(&mut self, sheet_name: &str) -> ForgeResult<BTreeMap<u32, CellColor>> {
        // Resolve sheet XML path first (needs &mut self.workbook), then
        // lazy-load styles, then extract. Ordering keeps borrows disjoint.
        let Some(path) = self.sheet_xml_path(sheet_name) else {
            return Ok(BTreeMap::new());
        };
        if self.styles.is_none() {
            self.styles = Some(xlsx_styles::load_styles(&self.zip_bytes)?);
        }
        let styles_owned: Option<&WorkbookStyles> = self.styles.as_ref().and_then(|s| s.as_ref());
        let Some(styles) = styles_owned else {
            return Ok(BTreeMap::new());
        };
        match xlsx_styles::extract_row_colors(&self.zip_bytes, &path, styles) {
            Ok(m) => Ok(m),
            Err(_) => Ok(BTreeMap::new()),
        }
    }
}

/// Detect which row holds the column headers. Scans rows 0..=9.
///
/// Score = count_of_nonempty_cells × distinctness_bonus − long_text_penalty.
/// Distinctness bonus favours rows with mostly-unique cells (true headers
/// rarely repeat column names). Long-text penalty discourages picking rows
/// that contain paragraph-length sentences (common in merged title banners).
fn detect_header_row(grid: &[Vec<String>]) -> usize {
    let scan_limit = grid.len().min(10);
    let mut best_score: i64 = -1;
    let mut best_row: usize = 0;
    for (i, row) in grid.iter().take(scan_limit).enumerate() {
        let non_empty: Vec<&String> = row.iter().filter(|c| !c.trim().is_empty()).collect();
        if non_empty.is_empty() {
            continue;
        }
        // Count unique non-empty values.
        let mut uniq = std::collections::HashSet::new();
        for c in &non_empty {
            uniq.insert(c.to_ascii_lowercase());
        }
        // Penalise rows with very-long cells (banners, merged titles).
        let long_count = non_empty
            .iter()
            .filter(|c| c.chars().count() > 80)
            .count() as i64;
        // Penalise rows with too few cells relative to row width (banners).
        let width = row.len().max(1) as i64;
        let fill_ratio = ((non_empty.len() as i64) * 100) / width;
        let score = (non_empty.len() as i64) * 10
            + (uniq.len() as i64) * 5
            + fill_ratio
            - long_count * 20;
        if score > best_score {
            best_score = score;
            best_row = i;
        }
    }
    best_row
}

#[derive(Debug, Clone)]
pub struct SheetData {
    pub name: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// Excel row number (1-indexed) for each entry in `rows`, in the same
    /// order. Lets us back-reference the sheet cell even after empty rows
    /// above the header were dropped.
    pub row_numbers: Vec<u32>,
    /// Excel row number (1-indexed) of the detected header row.
    pub header_row_number: u32,
    /// Dominant fill color per Excel row number. Empty when no fills present.
    pub row_colors: BTreeMap<u32, CellColor>,
}

impl SheetData {
    fn empty(name: &str) -> Self {
        Self {
            name: name.into(),
            headers: Vec::new(),
            rows: Vec::new(),
            row_numbers: Vec::new(),
            header_row_number: 1,
            row_colors: BTreeMap::new(),
        }
    }
}

fn cell_to_string(c: &Data) -> String {
    let raw = match c {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                format!("{}", f)
            }
        }
        Data::Int(i) => format!("{}", i),
        Data::Bool(b) => format!("{}", b),
        Data::DateTime(d) => format!("{}", d.as_f64()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#ERROR:{:?}", e),
    };
    sanitize_cell_text(&raw)
}

/// Normalise cell text for search + LLM consumption.
///
/// Real-world Excel cells routinely contain:
/// - `U+FFFD` replacement char from round-tripping a legacy-encoded byte
/// - `U+00A0` non-breaking space as a visual indent (`Alt+0160`)
/// - zero-width joiners / BOM (`U+200B`, `U+200C`, `U+200D`, `U+FEFF`)
/// - soft hyphens (`U+00AD`) invisible in Excel but byte-present
/// - control characters below `0x20` except tab and newline
///
/// None of these carry meaning for retrieval and they waste tokens in LLM
/// prompts. Replace with regular space, then collapse internal whitespace
/// and trim.
pub fn sanitize_cell_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_was_space = false;
    for ch in s.chars() {
        let cleaned = match ch {
            '\u{FFFD}' => ' ', // replacement char — unknown source encoding
            '\u{00A0}' => ' ', // non-breaking space
            '\u{00AD}' => continue, // soft hyphen
            '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' => continue, // ZWSP/ZWNJ/ZWJ/BOM
            c if c == '\t' || c == '\n' => c,
            c if (c as u32) < 0x20 => ' ', // other C0 controls
            c => c,
        };
        if cleaned == ' ' {
            if prev_was_space {
                continue;
            }
            prev_was_space = true;
        } else {
            prev_was_space = false;
        }
        out.push(cleaned);
    }
    out.trim().to_string()
}


/// Substring check (case-insensitive) against the skip patterns. E.g.
/// `should_skip_sheet("Requirements_Drafts_v3", &["_drafts".into()]) == true`.
pub fn should_skip_sheet(name: &str, patterns: &[String]) -> bool {
    let lc = name.to_ascii_lowercase();
    patterns.iter().any(|p| lc.contains(&p.to_ascii_lowercase()))
}

/// Slugify a string for use as a frame/story slug.
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "row".into()
    } else {
        out
    }
}

// ───────────────────────── grounding-mode ingest ─────────────────────────

/// Write one frame per non-empty row. Frame content is a flat JSON object
/// `{header1: cell1, header2: cell2, ...}`. Tag format:
/// `xlsx:<sheet-slug>:<excel-row-number>`. Fill color (when present) is
/// embedded in the JSON as a `_color` key and as a `color:<label>` tag.
pub fn ingest_sheet_as_grounding(
    said: &mut sca_core::said_file::SaidFile,
    sheet: &SheetData,
    authority_tag: &str,
    project_name: &str,
) -> ForgeResult<u64> {
    let mut count = 0u64;
    for (i, row) in sheet.rows.iter().enumerate() {
        let excel_row = sheet
            .row_numbers
            .get(i)
            .copied()
            .unwrap_or((sheet.header_row_number) + 1 + (i as u32));

        // Build JSON object, prefixing meta fields with "_" to keep them
        // separate from real columns.
        let mut obj = serde_json::Map::new();
        obj.insert(
            "_row".into(),
            serde_json::Value::Number(serde_json::Number::from(excel_row)),
        );
        obj.insert(
            "_sheet".into(),
            serde_json::Value::String(sheet.name.clone()),
        );
        let color_info = sheet.row_colors.get(&excel_row).cloned();
        if let Some(c) = &color_info {
            if c.is_meaningful() {
                obj.insert(
                    "_color_hex".into(),
                    serde_json::Value::String(c.hex.clone().unwrap_or_default()),
                );
                if let Some(label) = &c.label {
                    obj.insert(
                        "_color".into(),
                        serde_json::Value::String(label.clone()),
                    );
                }
            }
        }
        for (j, header) in sheet.headers.iter().enumerate() {
            let val = row.get(j).cloned().unwrap_or_default();
            let key = if header.is_empty() {
                format!("col_{}", j + 1)
            } else {
                header.clone()
            };
            obj.insert(key, serde_json::Value::String(val));
        }

        let content = serde_json::Value::Object(obj).to_string();
        let tag = format!("xlsx:{}:{}", slugify(&sheet.name), excel_row);
        let doc_id = format!("forge-xlsx:{}:{}", slugify(&sheet.name), excel_row);
        let mut tags = vec![
            tag.clone(),
            authority_tag.to_string(),
            format!("forge-project:{}", project_name),
            "forge-kind:xlsx-row".to_string(),
        ];
        if let Some(c) = &color_info {
            if c.is_meaningful() {
                if let Some(label) = &c.label {
                    tags.push(format!("color:{}", label));
                }
                if let Some(hex) = &c.hex {
                    tags.push(format!("color-hex:{}", hex.trim_start_matches('#')));
                }
            }
        }
        said.remember_with_pillar(
            Some(&doc_id),
            &content,
            Some(&tag),
            sca_core::frames::Pillar::External,
            tags,
        );
        count += 1;
    }
    Ok(count)
}

// ───────────────────────── story-per-row ─────────────────────────

/// Convert each row into a `Story` for `forge run` to iterate. Complements
/// (doesn't replace) `ingest_sheet_as_grounding` — the caller usually runs
/// both so the row data is searchable AND generates stories.
pub fn extract_stories(
    sheet: &SheetData,
    directive_hash: &str,
) -> Vec<crate::Story> {
    let mut out = Vec::new();
    for (i, row) in sheet.rows.iter().enumerate() {
        let slug = pick_slug(&sheet.headers, row, i);
        let title = pick_title(&sheet.headers, row);
        let raw_text = row_to_prose(&sheet.headers, row);
        let mut fields = std::collections::BTreeMap::new();
        for (h, v) in sheet.headers.iter().zip(row.iter()) {
            if h.is_empty() {
                continue;
            }
            fields.insert(h.clone(), serde_json::Value::String(v.clone()));
        }
        let excel_row = sheet.row_numbers.get(i).copied().unwrap_or(i as u32 + 2);
        if let Some(c) = sheet.row_colors.get(&excel_row) {
            if c.is_meaningful() {
                if let Some(label) = &c.label {
                    fields.insert(
                        "_color".into(),
                        serde_json::Value::String(label.clone()),
                    );
                }
            }
        }
        out.push(crate::Story {
            slug,
            title,
            raw_text,
            kind: crate::StoryKind::TableRow,
            fields,
            directive_hash: directive_hash.into(),
            source_adapter: "xlsx".into(),
            source_anchor: format!("sheet:{}:row:{}", sheet.name, excel_row),
        });
    }
    out
}

fn pick_slug(headers: &[String], row: &[String], fallback_idx: usize) -> String {
    let id_candidates = ["id", "key", "code", "ref", "req_id", "story_id", "ticket"];
    for (i, h) in headers.iter().enumerate() {
        let hl = h.to_ascii_lowercase();
        if id_candidates.contains(&hl.as_str()) || hl.ends_with("_id") {
            if let Some(v) = row.get(i) {
                if !v.trim().is_empty() {
                    return slugify(v);
                }
            }
        }
    }
    format!("row-{}", fallback_idx + 2)
}

fn pick_title(headers: &[String], row: &[String]) -> String {
    let t_candidates = ["title", "name", "story", "description", "summary"];
    for (i, h) in headers.iter().enumerate() {
        if t_candidates.contains(&h.to_ascii_lowercase().as_str()) {
            if let Some(v) = row.get(i) {
                if !v.trim().is_empty() {
                    return v.clone();
                }
            }
        }
    }
    row.iter()
        .find(|s| !s.trim().is_empty())
        .cloned()
        .unwrap_or_default()
}

fn row_to_prose(headers: &[String], row: &[String]) -> String {
    headers
        .iter()
        .zip(row.iter())
        .filter(|(_, v)| !v.trim().is_empty())
        .map(|(h, v)| {
            if h.is_empty() {
                v.clone()
            } else {
                format!("{}: {}", h, v)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Preview helper — renders the first `max_rows` of each sheet as a Markdown
/// table. For Q5 mode=Preview; not called by sync directly.
pub fn preview_first_rows(path: &Path, max_rows: usize) -> ForgeResult<String> {
    let mut reader = XlsxReader::open(path)?;
    let mut out = String::new();
    for sheet_name in reader.sheet_names() {
        out.push_str(&format!("## Sheet: {}\n\n", sheet_name));
        let sheet = reader.read_sheet(&sheet_name)?;
        if sheet.headers.is_empty() {
            out.push_str("(empty)\n\n");
            continue;
        }
        out.push_str("| ");
        out.push_str(&sheet.headers.join(" | "));
        out.push_str(" |\n| ");
        out.push_str(&vec!["---"; sheet.headers.len()].join(" | "));
        out.push_str(" |\n");
        for row in sheet.rows.iter().take(max_rows) {
            out.push_str("| ");
            out.push_str(&row.join(" | "));
            out.push_str(" |\n");
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_skip_sheet_matches_substrings_case_insensitive() {
        let skip = vec![
            "_drafts".to_string(),
            "_backup".to_string(),
            "~temp".to_string(),
        ];
        assert!(should_skip_sheet("Sheet1_Drafts", &skip));
        assert!(should_skip_sheet("Apr_Backup", &skip));
        assert!(should_skip_sheet("~temp1", &skip));
        assert!(!should_skip_sheet("Requirements", &skip));
        assert!(should_skip_sheet("reQs_DRAFTS_2026", &skip));
    }

    #[test]
    fn slugify_normalises_to_kebab() {
        assert_eq!(slugify("REQ-001"), "req-001");
        assert_eq!(slugify("Freeze Cardholder!"), "freeze-cardholder");
        assert_eq!(slugify("  -- "), "row");
        assert_eq!(slugify(""), "row");
    }

    #[test]
    fn pick_slug_from_id_column() {
        let headers = vec!["id".into(), "title".into()];
        let row = vec!["REQ-001".into(), "Freeze".into()];
        assert_eq!(pick_slug(&headers, &row, 0), "req-001");
    }

    #[test]
    fn pick_slug_from_story_id_column() {
        let headers = vec!["story_id".into(), "text".into()];
        let row = vec!["S42".into(), "hello".into()];
        assert_eq!(pick_slug(&headers, &row, 0), "s42");
    }

    #[test]
    fn pick_slug_falls_back_to_row_number() {
        let headers = vec!["x".into(), "y".into()];
        let row = vec!["a".into(), "b".into()];
        assert_eq!(pick_slug(&headers, &row, 5), "row-7"); // +2 offset
    }

    fn fixture_sheet(name: &str, headers: Vec<String>, rows: Vec<Vec<String>>) -> SheetData {
        let row_count = rows.len();
        SheetData {
            name: name.into(),
            headers,
            rows,
            row_numbers: (2..2 + row_count as u32).collect(),
            header_row_number: 1,
            row_colors: BTreeMap::new(),
        }
    }

    #[test]
    fn extract_stories_preserves_per_column_fields() {
        let sheet = fixture_sheet(
            "Reqs",
            vec!["id".into(), "title".into(), "priority".into()],
            vec![
                vec!["REQ-001".into(), "Freeze card".into(), "high".into()],
                vec!["REQ-002".into(), "BIN lookup".into(), "medium".into()],
            ],
        );
        let stories = extract_stories(&sheet, "a1b2c");
        assert_eq!(stories.len(), 2);
        assert_eq!(stories[0].slug, "req-001");
        assert_eq!(stories[0].title, "Freeze card");
        assert_eq!(stories[0].kind, crate::StoryKind::TableRow);
        assert_eq!(stories[0].source_adapter, "xlsx");
        assert_eq!(stories[0].source_anchor, "sheet:Reqs:row:2");
        assert!(stories[0].raw_text.contains("priority: high"));
        assert_eq!(
            stories[1].fields.get("priority").and_then(|v| v.as_str()),
            Some("medium")
        );
    }

    #[test]
    fn detect_header_row_picks_the_densest_unique_row() {
        // Row 0: single-cell banner (merged-title simulation after unfolding)
        // Row 1: (empty)
        // Row 2: 4-cell header row
        // Row 3+: data rows
        let grid = vec![
            vec!["Detailed Requirements".into(), "".into(), "".into(), "".into()],
            vec!["".into(), "".into(), "".into(), "".into()],
            vec!["id".into(), "title".into(), "status".into(), "priority".into()],
            vec!["R1".into(), "Freeze".into(), "open".into(), "high".into()],
            vec!["R2".into(), "BIN".into(), "done".into(), "low".into()],
        ];
        assert_eq!(detect_header_row(&grid), 2);
    }

    #[test]
    fn detect_header_row_defaults_to_row_zero_when_tied() {
        let grid = vec![
            vec!["a".into(), "b".into()],
            vec!["c".into(), "d".into()],
        ];
        assert_eq!(detect_header_row(&grid), 0);
    }

    #[test]
    fn cell_to_string_handles_common_types() {
        use calamine::Data;
        assert_eq!(cell_to_string(&Data::Empty), "");
        assert_eq!(cell_to_string(&Data::String("hi".into())), "hi");
        assert_eq!(cell_to_string(&Data::Int(42)), "42");
        assert_eq!(cell_to_string(&Data::Float(3.14)), "3.14");
        // Whole-number float coerces to int form (spreadsheets store all numbers as float)
        assert_eq!(cell_to_string(&Data::Float(100.0)), "100");
        assert_eq!(cell_to_string(&Data::Bool(true)), "true");
        assert_eq!(cell_to_string(&Data::DateTimeIso("2026-04-24".into())), "2026-04-24");
    }

    #[test]
    fn sanitize_cell_text_strips_replacement_char_and_nbsp() {
        // Real-world Excel payload: replacement char + NBSP before "POST"
        assert_eq!(sanitize_cell_text("\u{FFFD}\u{00A0}POST /user/create"), "POST /user/create");
    }

    #[test]
    fn sanitize_cell_text_drops_zero_width_and_soft_hyphen() {
        assert_eq!(sanitize_cell_text("he\u{200B}llo\u{00AD}world"), "helloworld");
        assert_eq!(sanitize_cell_text("\u{FEFF}leading BOM"), "leading BOM");
    }

    #[test]
    fn sanitize_cell_text_collapses_internal_whitespace_and_trims() {
        assert_eq!(sanitize_cell_text("  foo   bar  "), "foo bar");
        assert_eq!(sanitize_cell_text("mix\u{00A0} \u{00A0}spaces"), "mix spaces");
    }

    #[test]
    fn sanitize_cell_text_preserves_tabs_and_newlines() {
        assert_eq!(sanitize_cell_text("line1\nline2\tcol"), "line1\nline2\tcol");
    }

    #[test]
    fn sanitize_cell_text_replaces_c0_controls_with_space() {
        // Bell + form-feed → collapsed to single space
        assert_eq!(sanitize_cell_text("a\x07b\x0Cc"), "a b c");
    }

    #[test]
    fn ingest_sheet_as_grounding_writes_one_frame_per_row() {
        use sca_core::said_file::{BrainMode, SaidFile};
        let tmp = tempfile::TempDir::new().unwrap();
        let said_path = tmp.path().join("t.said");
        let mut said = SaidFile::create_with_mode(&said_path, BrainMode::Portable);
        let sheet = fixture_sheet(
            "Requirements",
            vec!["id".into(), "description".into()],
            vec![
                vec!["R1".into(), "Freeze cardholder with audit".into()],
                vec!["R2".into(), "BIN lookup ISO 7812".into()],
                vec!["R3".into(), "Address verification".into()],
            ],
        );
        let n = ingest_sheet_as_grounding(
            &mut said,
            &sheet,
            "authority:requested:requirements",
            "w",
        )
        .unwrap();
        assert_eq!(n, 3);
        let frames = said.frames.get_all_frames_with_pending();
        let tagged = frames
            .iter()
            .filter(|m| {
                m.tags.iter().any(|t| t.starts_with("xlsx:"))
                    && m.tags
                        .iter()
                        .any(|t| t.starts_with("authority:requested:requirements"))
            })
            .count();
        assert_eq!(tagged, 3);
    }

    #[test]
    fn ingest_row_with_color_adds_color_tag_and_json_key() {
        use sca_core::said_file::{BrainMode, SaidFile};
        let tmp = tempfile::TempDir::new().unwrap();
        let said_path = tmp.path().join("t.said");
        let mut said = SaidFile::create_with_mode(&said_path, BrainMode::Portable);
        let mut sheet = fixture_sheet(
            "Reqs",
            vec!["id".into(), "status".into()],
            vec![vec!["R1".into(), "done".into()]],
        );
        sheet.row_colors.insert(
            2,
            CellColor {
                hex: Some("#70AD47".into()),
                label: Some("dark-green".into()),
            },
        );
        let n =
            ingest_sheet_as_grounding(&mut said, &sheet, "authority:requested:requirements", "w")
                .unwrap();
        assert_eq!(n, 1);
        let frames = said.frames.get_all_frames_with_pending();
        assert!(
            frames
                .iter()
                .any(|m| m.tags.iter().any(|t| t == "color:dark-green")),
            "expected color:dark-green tag; got: {:?}",
            frames.iter().flat_map(|m| m.tags.iter()).collect::<Vec<_>>()
        );
    }
}
