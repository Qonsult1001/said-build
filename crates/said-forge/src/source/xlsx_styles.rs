//! Parse `xl/styles.xml` + per-cell style indices to extract fill colors.
//!
//! calamine 0.34 exposes cell values but not formatting. For forge we care
//! about fill color (the "green rows" / "red rows" semantic signal users
//! use in practice) so we parse styles.xml ourselves.
//!
//! Pipeline:
//! 1. Parse `xl/styles.xml`:
//!    - `<indexedColors>` → legacy 16-color palette (if present)
//!    - `<fills>` → each `<fill>` has `<patternFill>` with optional
//!      `<fgColor>` + `<bgColor>` carrying either `rgb="AARRGGBB"`,
//!      `indexed="n"` (legacy palette), or `theme="n" tint="..."` (theme).
//!    - `<cellXfs>` → each `<xf>` has `fillId="n"` pointing into fills array.
//! 2. Parse each sheet XML:
//!    - `<c r="B5" s="42">` → style index, lookup via cellXfs → fillId → fill.
//!    - Build `BTreeMap<row_number (1-indexed), CellColor>` per sheet.
//!      Row color = most-common non-white cell color in the row.
//!
//! Theme colors are resolved to approximate hex values using Office's
//! default theme palette (theme1.xml is rarely patched by users).

#![cfg(feature = "forge-xlsx")]

use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use zip::ZipArchive;

use crate::{ForgeError, ForgeResult};

/// A fill color we've decoded from styles.xml. Hex in `#RRGGBB` form. `None`
/// means no fill / fully transparent / default white.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CellColor {
    pub hex: Option<String>,
    /// Short semantic label derived from the hex (e.g. "green", "red",
    /// "yellow", "white"). Useful for agent-facing tags without the agent
    /// needing to understand hex.
    pub label: Option<String>,
}

impl CellColor {
    pub fn is_meaningful(&self) -> bool {
        match &self.hex {
            None => false,
            Some(h) => !matches!(h.as_str(), "#FFFFFF" | "#000000" | "#"),
        }
    }
}

/// Per-workbook styles parsed once, used for every sheet.
#[derive(Debug, Default)]
pub struct WorkbookStyles {
    /// `fills[i]` → CellColor (fgColor preferred, falls back to bgColor).
    pub fills: Vec<CellColor>,
    /// `cell_xfs[i].fill_id` → index into fills.
    pub cell_xfs: Vec<u32>,
}

/// Load styles from the workbook zip. Returns `Ok(None)` if styles.xml is
/// missing (unusual but possible for non-Excel producers).
pub fn load_styles(zip_bytes: &[u8]) -> ForgeResult<Option<WorkbookStyles>> {
    let mut archive = ZipArchive::new(Cursor::new(zip_bytes)).map_err(|e| ForgeError::Parse {
        path: "<xlsx>".into(),
        message: format!("zip open: {}", e),
    })?;
    let styles_bytes = match archive.by_name("xl/styles.xml") {
        Ok(mut f) => {
            let mut b = Vec::new();
            f.read_to_end(&mut b).map_err(|e| ForgeError::Io {
                path: "xl/styles.xml".into(),
                cause: e,
            })?;
            b
        }
        Err(_) => return Ok(None),
    };
    parse_styles(&styles_bytes).map(Some)
}

fn parse_styles(bytes: &[u8]) -> ForgeResult<WorkbookStyles> {
    let mut reader = Reader::from_reader(bytes);
    reader.trim_text(true);
    let mut buf = Vec::new();

    let mut fills: Vec<CellColor> = Vec::new();
    let mut cell_xfs: Vec<u32> = Vec::new();
    let mut indexed: Vec<String> = default_indexed_palette();

    let mut in_fills = false;
    let mut in_cellxfs = false;
    let mut in_indexed = false;
    let mut current_fill: Option<CellColor> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"fills" => in_fills = true,
                b"cellXfs" => in_cellxfs = true,
                b"indexedColors" => {
                    in_indexed = true;
                    indexed.clear();
                }
                b"fill" if in_fills => current_fill = Some(CellColor::default()),
                b"patternFill" if in_fills => {
                    if let Some(fill) = current_fill.as_mut() {
                        for a in e.attributes().with_checks(false).flatten() {
                            if a.key.as_ref() == b"patternType" {
                                if let Ok(v) = a.unescape_value() {
                                    if v == "none" {
                                        fill.hex = None;
                                    }
                                }
                            }
                        }
                    }
                }
                b"xf" if in_cellxfs => {
                    // `<xf ...>` with nested alignment/protection. fillId is
                    // on the xf itself — capture it, then keep reading until
                    // </xf> (handled by the End branch; no state needed since
                    // nested elements don't affect fill).
                    let mut fill_id = 0u32;
                    for a in e.attributes().with_checks(false).flatten() {
                        if a.key.as_ref() == b"fillId" {
                            if let Ok(v) = a.unescape_value() {
                                fill_id = v.parse().unwrap_or(0);
                            }
                        }
                    }
                    cell_xfs.push(fill_id);
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"rgbColor" if in_indexed => {
                    for a in e.attributes().with_checks(false).flatten() {
                        if a.key.as_ref() == b"rgb" {
                            if let Ok(v) = a.unescape_value() {
                                indexed.push(format!("#{}", strip_alpha(&v)));
                            }
                        }
                    }
                }
                b"fgColor" | b"bgColor" if in_fills => {
                    let is_fg = e.local_name().as_ref() == b"fgColor";
                    // Prefer fgColor. Only overwrite with bgColor if fgColor empty.
                    if let Some(fill) = current_fill.as_mut() {
                        if is_fg || fill.hex.is_none() {
                            if let Some(hex) = color_attr_to_hex(&e, &indexed) {
                                fill.hex = Some(hex);
                            }
                        }
                    }
                }
                b"patternFill" if in_fills => {
                    if let Some(fill) = current_fill.as_mut() {
                        for a in e.attributes().with_checks(false).flatten() {
                            if a.key.as_ref() == b"patternType" {
                                if let Ok(v) = a.unescape_value() {
                                    if v == "none" {
                                        fill.hex = None;
                                    }
                                }
                            }
                        }
                    }
                }
                b"xf" if in_cellxfs => {
                    // Self-closing <xf .../> (no nested alignment/protection).
                    let mut fill_id = 0u32;
                    for a in e.attributes().with_checks(false).flatten() {
                        if a.key.as_ref() == b"fillId" {
                            if let Ok(v) = a.unescape_value() {
                                fill_id = v.parse().unwrap_or(0);
                            }
                        }
                    }
                    cell_xfs.push(fill_id);
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"fills" => in_fills = false,
                b"cellXfs" => in_cellxfs = false,
                b"indexedColors" => in_indexed = false,
                b"fill" if in_fills => {
                    if let Some(mut fill) = current_fill.take() {
                        fill.label = fill
                            .hex
                            .as_deref()
                            .and_then(hex_to_color_name)
                            .map(String::from);
                        fills.push(fill);
                    }
                }
                b"xf" if in_cellxfs => {
                    // Handled in Event::Start branch for self-closing xfs.
                    // This branch covers <xf>...</xf> with nested alignment.
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(ForgeError::Parse {
                    path: "styles.xml".into(),
                    message: format!("xml: {}", e),
                })
            }
            _ => {}
        }
        buf.clear();
    }

    let _ = indexed; // retained for future exposure; already baked into fills via color_attr_to_hex
    Ok(WorkbookStyles { fills, cell_xfs })
}

fn color_attr_to_hex(e: &quick_xml::events::BytesStart, indexed: &[String]) -> Option<String> {
    let mut rgb: Option<String> = None;
    let mut indexed_idx: Option<usize> = None;
    let mut theme: Option<usize> = None;
    for a in e.attributes().with_checks(false).flatten() {
        match a.key.as_ref() {
            b"rgb" => rgb = a.unescape_value().ok().map(|c| c.to_string()),
            b"indexed" => {
                indexed_idx = a
                    .unescape_value()
                    .ok()
                    .and_then(|c| c.parse::<usize>().ok());
            }
            b"theme" => {
                theme = a.unescape_value().ok().and_then(|c| c.parse::<usize>().ok());
            }
            _ => {}
        }
    }
    if let Some(r) = rgb {
        return Some(format!("#{}", strip_alpha(&r)));
    }
    if let Some(i) = indexed_idx {
        return indexed.get(i).cloned();
    }
    if let Some(t) = theme {
        return Some(default_theme_color(t));
    }
    None
}

fn strip_alpha(rgb: &str) -> String {
    // Excel writes ARGB ("FF00FF00"). Drop the first two chars.
    if rgb.len() == 8 {
        rgb[2..].to_string()
    } else {
        rgb.to_string()
    }
}

/// Excel's legacy indexed palette (positions 0..63). Copied from ECMA-376
/// §18.8.27. Users can override with `<indexedColors>`; we merge when present.
fn default_indexed_palette() -> Vec<String> {
    vec![
        "#000000", "#FFFFFF", "#FF0000", "#00FF00", "#0000FF", "#FFFF00", "#FF00FF", "#00FFFF",
        "#000000", "#FFFFFF", "#FF0000", "#00FF00", "#0000FF", "#FFFF00", "#FF00FF", "#00FFFF",
        "#800000", "#008000", "#000080", "#808000", "#800080", "#008080", "#C0C0C0", "#808080",
        "#9999FF", "#993366", "#FFFFCC", "#CCFFFF", "#660066", "#FF8080", "#0066CC", "#CCCCFF",
        "#000080", "#FF00FF", "#FFFF00", "#00FFFF", "#800080", "#800000", "#008080", "#0000FF",
        "#00CCFF", "#CCFFFF", "#CCFFCC", "#FFFF99", "#99CCFF", "#FF99CC", "#CC99FF", "#FFCC99",
        "#3366FF", "#33CCCC", "#99CC00", "#FFCC00", "#FF9900", "#FF6600", "#666699", "#969696",
        "#003366", "#339966", "#003300", "#333300", "#993300", "#993366", "#333399", "#333333",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// Approximate Office default theme colors (theme1.xml). Indexes 0..11
/// follow the `<clrScheme>` child order: bg1, tx1, bg2, tx2, accent1..accent6,
/// hlink, folHlink.
fn default_theme_color(theme_idx: usize) -> String {
    match theme_idx {
        0 => "#FFFFFF".into(), // lt1 / bg1
        1 => "#000000".into(), // dk1 / tx1
        2 => "#E7E6E6".into(), // lt2 / bg2
        3 => "#44546A".into(), // dk2 / tx2
        4 => "#4472C4".into(), // accent1 — blue
        5 => "#ED7D31".into(), // accent2 — orange
        6 => "#A5A5A5".into(), // accent3 — grey
        7 => "#FFC000".into(), // accent4 — yellow
        8 => "#5B9BD5".into(), // accent5 — lt blue
        9 => "#70AD47".into(), // accent6 — green
        10 => "#0563C1".into(), // hlink
        11 => "#954F72".into(), // folHlink
        _ => "#FFFFFF".into(),
    }
}

/// Collapse a hex color to a human semantic label. Uses HSL-like bucketing
/// with Office's common theme colors as anchors.
pub fn hex_to_color_name(hex: &str) -> Option<&'static str> {
    let s = hex.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;

    let avg = ((r as u32) + (g as u32) + (b as u32)) / 3;
    // Near-white / near-black
    if r > 240 && g > 240 && b > 240 {
        return Some("white");
    }
    if r < 30 && g < 30 && b < 30 {
        return Some("black");
    }
    // Grey if R/G/B within 20 of each other
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if (max as i32) - (min as i32) < 25 {
        if avg > 180 {
            return Some("light-grey");
        }
        if avg > 100 {
            return Some("grey");
        }
        return Some("dark-grey");
    }
    // Dominant channel bucketing — cast to i32 to avoid u8 overflow on +20.
    let ri = r as i32;
    let gi = g as i32;
    let bi = b as i32;
    if gi > ri + 20 && gi > bi + 20 {
        return Some(if g > 200 { "green" } else { "dark-green" });
    }
    if ri > gi + 20 && ri > bi + 20 {
        return Some(if g > 100 { "orange" } else { "red" });
    }
    if bi > ri + 20 && bi > gi + 20 {
        return Some(if r > 100 { "purple" } else { "blue" });
    }
    if r > 200 && g > 200 && b < 120 {
        return Some("yellow");
    }
    if r > 200 && g < 130 && b > 130 {
        return Some("pink");
    }
    None
}

// ───────────────────────── sheet-level color resolution ─────────────────────────

/// Parse a sheet XML and return `row_number (1-indexed) → CellColor`. Row
/// color = the most-common non-neutral cell color in that row; if all cells
/// are neutral (white / grey / no fill), the row gets a default empty color.
pub fn extract_row_colors(
    zip_bytes: &[u8],
    sheet_path: &str,
    styles: &WorkbookStyles,
) -> ForgeResult<BTreeMap<u32, CellColor>> {
    let mut archive = ZipArchive::new(Cursor::new(zip_bytes)).map_err(|e| ForgeError::Parse {
        path: "<xlsx>".into(),
        message: format!("zip open: {}", e),
    })?;
    let mut xml_bytes = Vec::new();
    {
        let mut f = archive.by_name(sheet_path).map_err(|e| ForgeError::Parse {
            path: sheet_path.into(),
            message: format!("sheet not in zip: {}", e),
        })?;
        f.read_to_end(&mut xml_bytes).map_err(|e| ForgeError::Io {
            path: sheet_path.into(),
            cause: e,
        })?;
    }

    let mut reader = Reader::from_reader(xml_bytes.as_slice());
    reader.trim_text(true);
    let mut buf = Vec::new();

    // row_number → histogram of colors
    let mut per_row: BTreeMap<u32, BTreeMap<String, u32>> = BTreeMap::new();
    let mut current_row: u32 = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"row" => {
                    for a in e.attributes().with_checks(false).flatten() {
                        if a.key.as_ref() == b"r" {
                            if let Ok(v) = a.unescape_value() {
                                current_row = v.parse().unwrap_or(0);
                            }
                        }
                    }
                }
                b"c" => {
                    let mut style_idx: Option<u32> = None;
                    for a in e.attributes().with_checks(false).flatten() {
                        if a.key.as_ref() == b"s" {
                            if let Ok(v) = a.unescape_value() {
                                style_idx = v.parse().ok();
                            }
                        }
                    }
                    if let Some(s) = style_idx {
                        if let Some(color) = resolve_color(styles, s) {
                            if color.is_meaningful() {
                                let hex = color.hex.clone().unwrap_or_default();
                                *per_row
                                    .entry(current_row)
                                    .or_default()
                                    .entry(hex)
                                    .or_insert(0) += 1;
                            }
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(ForgeError::Parse {
                    path: sheet_path.into(),
                    message: format!("xml: {}", e),
                })
            }
            _ => {}
        }
        buf.clear();
    }

    // Collapse histograms: pick the most-common color per row.
    let mut out = BTreeMap::new();
    for (row, hist) in per_row {
        let (dominant_hex, _) = hist
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .unwrap_or_default();
        let label = hex_to_color_name(&dominant_hex).map(String::from);
        out.insert(
            row,
            CellColor {
                hex: if dominant_hex.is_empty() {
                    None
                } else {
                    Some(dominant_hex)
                },
                label,
            },
        );
    }
    Ok(out)
}

fn resolve_color(styles: &WorkbookStyles, xf_idx: u32) -> Option<CellColor> {
    let fill_id = styles.cell_xfs.get(xf_idx as usize).copied()?;
    styles.fills.get(fill_id as usize).cloned()
}

/// Resolve the sheet-XML path for a given sheet name by reading
/// `xl/workbook.xml` + `xl/_rels/workbook.xml.rels`. Sheet names DON'T map
/// 1:1 to `sheet{N}.xml` filenames — relationships do. Returns `None` for
/// sheets that aren't backed by a worksheet (chart/dialog).
pub fn resolve_sheet_path(zip_bytes: &[u8], sheet_name: &str) -> Option<String> {
    let mut archive = ZipArchive::new(Cursor::new(zip_bytes)).ok()?;
    // Step 1: read workbook.xml — get ordered list of (name, r:id) pairs.
    let mut wb_bytes = Vec::new();
    archive
        .by_name("xl/workbook.xml")
        .ok()?
        .read_to_end(&mut wb_bytes)
        .ok()?;
    let sheet_rels = parse_workbook_sheet_refs(&wb_bytes);
    let rel_id = sheet_rels
        .iter()
        .find(|(n, _)| n == sheet_name)
        .map(|(_, rid)| rid.clone())?;

    // Step 2: read workbook.xml.rels — map r:id → Target path.
    let mut rels_bytes = Vec::new();
    archive
        .by_name("xl/_rels/workbook.xml.rels")
        .ok()?
        .read_to_end(&mut rels_bytes)
        .ok()?;
    let target = parse_rel_target(&rels_bytes, &rel_id)?;
    // Target is usually like `worksheets/sheet2.xml` — prepend `xl/` when
    // it's relative.
    if target.starts_with("/xl/") {
        Some(target.trim_start_matches('/').to_string())
    } else if target.starts_with("xl/") {
        Some(target)
    } else {
        Some(format!("xl/{}", target))
    }
}

fn parse_workbook_sheet_refs(bytes: &[u8]) -> Vec<(String, String)> {
    let mut reader = Reader::from_reader(bytes);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.local_name().as_ref() == b"sheet" =>
            {
                let mut name = None;
                let mut rid = None;
                for a in e.attributes().with_checks(false).flatten() {
                    let k = a.key.as_ref();
                    // Match both `r:id` and the local-name `id` (strip ns).
                    if k == b"name" {
                        name = a.unescape_value().ok().map(|c| c.to_string());
                    } else if k == b"r:id" || k == b"relationships:id" {
                        rid = a.unescape_value().ok().map(|c| c.to_string());
                    }
                }
                if let (Some(n), Some(r)) = (name, rid) {
                    out.push((n, r));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

fn parse_rel_target(bytes: &[u8], rel_id: &str) -> Option<String> {
    let mut reader = Reader::from_reader(bytes);
    reader.trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.local_name().as_ref() == b"Relationship" =>
            {
                let mut id = None;
                let mut target = None;
                for a in e.attributes().with_checks(false).flatten() {
                    match a.key.as_ref() {
                        b"Id" => id = a.unescape_value().ok().map(|c| c.to_string()),
                        b"Target" => target = a.unescape_value().ok().map(|c| c.to_string()),
                        _ => {}
                    }
                }
                if let (Some(i), Some(t)) = (id, target) {
                    if i == rel_id {
                        return Some(t);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}

/// Parse `<mergeCells><mergeCell ref="A1:C3"/></mergeCells>` from a sheet XML.
/// Returns `[(start_row, start_col), (end_row, end_col)]` pairs, 0-indexed.
pub fn extract_merged_ranges(
    zip_bytes: &[u8],
    sheet_path: &str,
) -> ForgeResult<Vec<((u32, u32), (u32, u32))>> {
    let mut archive = ZipArchive::new(Cursor::new(zip_bytes)).map_err(|e| ForgeError::Parse {
        path: "<xlsx>".into(),
        message: format!("zip open: {}", e),
    })?;
    let mut xml_bytes = Vec::new();
    {
        let mut f = archive.by_name(sheet_path).map_err(|e| ForgeError::Parse {
            path: sheet_path.into(),
            message: format!("sheet not in zip: {}", e),
        })?;
        f.read_to_end(&mut xml_bytes).map_err(|e| ForgeError::Io {
            path: sheet_path.into(),
            cause: e,
        })?;
    }
    let mut reader = Reader::from_reader(xml_bytes.as_slice());
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.local_name().as_ref() == b"mergeCell" =>
            {
                for a in e.attributes().with_checks(false).flatten() {
                    if a.key.as_ref() == b"ref" {
                        if let Ok(v) = a.unescape_value() {
                            if let Some(range) = parse_excel_range(&v) {
                                out.push(range);
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// `"B3:D5"` → `((2, 1), (4, 3))` (0-indexed row, col).
pub fn parse_excel_range(s: &str) -> Option<((u32, u32), (u32, u32))> {
    let mut parts = s.split(':');
    let start = parse_excel_ref(parts.next()?)?;
    let end = parts.next().and_then(parse_excel_ref).unwrap_or(start);
    Some((start, end))
}

/// `"B3"` → `(2, 1)` (0-indexed row, col).
fn parse_excel_ref(s: &str) -> Option<(u32, u32)> {
    let mut col_end = 0;
    for (i, ch) in s.chars().enumerate() {
        if ch.is_ascii_alphabetic() {
            col_end = i + 1;
        } else {
            break;
        }
    }
    if col_end == 0 {
        return None;
    }
    let col_part = &s[..col_end];
    let row_part = &s[col_end..];
    let mut col: u32 = 0;
    for ch in col_part.chars() {
        col = col * 26 + (ch.to_ascii_uppercase() as u32 - b'A' as u32 + 1);
    }
    let col = col.checked_sub(1)?;
    let row: u32 = row_part.parse().ok()?;
    Some((row.checked_sub(1)?, col))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_green_maps_to_green_label() {
        assert_eq!(hex_to_color_name("#00FF00"), Some("green"));
        // Office accent6 — sits in the dark-green bucket (G=0xAD is below 200).
        assert_eq!(hex_to_color_name("#70AD47"), Some("dark-green"));
        assert_eq!(hex_to_color_name("#FF0000"), Some("red"));
        assert_eq!(hex_to_color_name("#FFFF00"), Some("yellow"));
        assert_eq!(hex_to_color_name("#FFFFFF"), Some("white"));
        assert_eq!(hex_to_color_name("#000000"), Some("black"));
    }

    #[test]
    fn strip_alpha_drops_first_two_chars() {
        assert_eq!(strip_alpha("FF00FF00"), "00FF00");
        assert_eq!(strip_alpha("00FF00"), "00FF00"); // already 6-char
    }

    #[test]
    fn cell_color_is_meaningful_ignores_white_and_none() {
        let white = CellColor {
            hex: Some("#FFFFFF".into()),
            label: Some("white".into()),
        };
        assert!(!white.is_meaningful());
        let none = CellColor::default();
        assert!(!none.is_meaningful());
        let green = CellColor {
            hex: Some("#70AD47".into()),
            label: Some("green".into()),
        };
        assert!(green.is_meaningful());
    }

    #[test]
    fn default_indexed_palette_has_64_entries() {
        assert_eq!(default_indexed_palette().len(), 64);
    }

    #[test]
    fn parse_excel_ref_handles_single_and_multi_letter_columns() {
        assert_eq!(parse_excel_ref("A1"), Some((0, 0)));
        assert_eq!(parse_excel_ref("B3"), Some((2, 1)));
        assert_eq!(parse_excel_ref("Z10"), Some((9, 25)));
        assert_eq!(parse_excel_ref("AA1"), Some((0, 26)));
        assert_eq!(parse_excel_ref("AB5"), Some((4, 27)));
    }

    #[test]
    fn parse_excel_range_handles_single_cell_and_ranges() {
        assert_eq!(parse_excel_range("A1"), Some(((0, 0), (0, 0))));
        assert_eq!(parse_excel_range("B3:D5"), Some(((2, 1), (4, 3))));
    }
}
