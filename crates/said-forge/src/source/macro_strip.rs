//! Strip Excel 4.0 macro sheets from `.xlsm` workbooks so calamine can
//! read the cell data.
//!
//! Background: calamine 0.34 hard-errors on any sheet whose `r:id` path
//! resolves outside `xl/worksheets/`, `xl/chartsheets/`, `xl/dialogsheets/`.
//! Macro-enabled workbooks add `xl/macrosheets/sheet*.xml` entries plus
//! `<sheet>` entries in `xl/workbook.xml` referencing them via relationships.
//! We don't need macro sheets — we only want the tabular data — so we
//! rewrite a cleaned copy to a temp file and pass that to calamine.
//!
//! The strip:
//! 1. Parse `xl/_rels/workbook.xml.rels` to find which relationship IDs
//!    target `macrosheets/...`.
//! 2. Parse `xl/workbook.xml`, drop any `<sheet>` whose `r:id` matches
//!    those relationships.
//! 3. Copy every other entry verbatim. Drop the macrosheets files + their
//!    relationship rows in the rels XML too.
//! 4. Write to a temp file and return its path. Caller owns the temp file
//!    handle (kept in a thread-local to stay alive for calamine's read).

#![cfg(feature = "forge-xlsx")]

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};
use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use zip::{write::FileOptions, ZipArchive, ZipWriter};

use crate::{ForgeError, ForgeResult};

/// Produce a cleaned copy of an `.xlsm` at a temp path. Returns the temp
/// PathBuf. The file is left on disk so the caller can pass it to calamine;
/// temp cleanup relies on `tempfile` crate.
pub fn strip_macrosheets_to_temp(src: &Path) -> ForgeResult<PathBuf> {
    let bytes = std::fs::read(src).map_err(|e| ForgeError::Io {
        path: src.display().to_string(),
        cause: e,
    })?;
    let cleaned = strip_macrosheets_bytes(&bytes)?;

    // Use NamedTempFile with a .xlsx suffix so calamine's extension-based
    // dispatcher routes to the xlsx reader (otherwise it'd see `.tmp`).
    let mut nf = tempfile::Builder::new()
        .prefix("forge-xlsx-clean-")
        .suffix(".xlsx")
        .tempfile()
        .map_err(|e| ForgeError::Io {
            path: "<tempfile>".into(),
            cause: e,
        })?;
    nf.write_all(&cleaned).map_err(|e| ForgeError::Io {
        path: "<tempfile>".into(),
        cause: e,
    })?;
    // Keep the file alive after TempPath drops. It's a temp — the OS will
    // clean it on process exit.
    let (_f, path) = nf.keep().map_err(|e| ForgeError::Validation(
        format!("temp keep: {}", e),
    ))?;
    Ok(path)
}

fn strip_macrosheets_bytes(input: &[u8]) -> ForgeResult<Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(input)).map_err(|e| ForgeError::Parse {
        path: "<xlsm>".into(),
        message: format!("zip open: {}", e),
    })?;

    // Step 1: identify macrosheet relationship IDs by reading xl/_rels/workbook.xml.rels.
    let rels_bytes = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let macrosheet_rel_ids = find_macrosheet_rel_ids(&rels_bytes)?;

    // Step 2: copy every entry to a new zip, rewriting workbook.xml and
    // workbook.xml.rels to drop macrosheet references, and skipping the
    // macrosheets/* entries entirely.
    let buffer: Vec<u8> = {
        let mut out = Cursor::new(Vec::<u8>::new());
        {
            let mut zw = ZipWriter::new(&mut out);
            let options: FileOptions =
                FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

            let mut names: Vec<String> = archive.file_names().map(String::from).collect();
            names.sort();
            for name in names {
                if name.starts_with("xl/macrosheets/") {
                    continue;
                }
                let mut zf = archive.by_name(&name).map_err(|e| ForgeError::Parse {
                    path: name.clone(),
                    message: format!("zip read: {}", e),
                })?;
                let mut data = Vec::new();
                zf.read_to_end(&mut data).map_err(|e| ForgeError::Io {
                    path: name.clone(),
                    cause: e,
                })?;

                let rewritten = if name == "xl/workbook.xml" {
                    strip_sheet_entries(&data, &macrosheet_rel_ids)?
                } else if name == "xl/_rels/workbook.xml.rels" {
                    strip_rels_entries(&data, &macrosheet_rel_ids)?
                } else {
                    data
                };

                zw.start_file(name, options).map_err(|e| ForgeError::Parse {
                    path: "<zip out>".into(),
                    message: format!("start_file: {}", e),
                })?;
                zw.write_all(&rewritten).map_err(|e| ForgeError::Io {
                    path: "<zip out>".into(),
                    cause: e,
                })?;
            }
            zw.finish().map_err(|e| ForgeError::Parse {
                path: "<zip out>".into(),
                message: format!("zip finish: {}", e),
            })?;
        }
        out.into_inner()
    };
    Ok(buffer)
}

fn read_entry(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> ForgeResult<Vec<u8>> {
    let mut zf = archive.by_name(name).map_err(|e| ForgeError::Parse {
        path: name.into(),
        message: format!("zip missing entry: {}", e),
    })?;
    let mut data = Vec::new();
    zf.read_to_end(&mut data).map_err(|e| ForgeError::Io {
        path: name.into(),
        cause: e,
    })?;
    Ok(data)
}

/// From `xl/_rels/workbook.xml.rels`, find every `<Relationship>` whose
/// Target points at `macrosheets/...` and return its Id.
fn find_macrosheet_rel_ids(rels_bytes: &[u8]) -> ForgeResult<HashSet<String>> {
    let mut reader = Reader::from_reader(rels_bytes);
    reader.trim_text(false);
    let mut ids = HashSet::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().as_ref() == b"Relationship" => {
                let mut id = None;
                let mut target = None;
                for a in e.attributes().with_checks(false).flatten() {
                    match a.key.as_ref() {
                        b"Id" => id = a.unescape_value().ok().map(|c| c.to_string()),
                        b"Target" => target = a.unescape_value().ok().map(|c| c.to_string()),
                        _ => {}
                    }
                }
                if let (Some(id), Some(t)) = (id, target) {
                    if t.contains("macrosheets/") {
                        ids.insert(id);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(ForgeError::Parse {
                    path: "workbook.xml.rels".into(),
                    message: format!("xml: {}", e),
                })
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(ids)
}

/// Rewrite `xl/workbook.xml`, dropping every `<sheet>` whose `r:id` matches
/// a macrosheet relationship id.
fn strip_sheet_entries(input: &[u8], macro_ids: &HashSet<String>) -> ForgeResult<Vec<u8>> {
    if macro_ids.is_empty() {
        return Ok(input.to_vec());
    }
    let mut reader = Reader::from_reader(input);
    reader.trim_text(false);
    let mut out = Vec::with_capacity(input.len());
    let mut writer = Writer::new(&mut out);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) if is_sheet(e) && sheet_has_macro_rid(e, macro_ids) => {
                // Skip self-closing <sheet .../>. For a Start variant we
                // additionally swallow until the matching End.
                if matches!(reader.read_event_into(&mut Vec::new()), Ok(Event::Eof)) {
                    break;
                }
                // Empty event — already consumed, continue.
            }
            Ok(Event::Eof) => break,
            Ok(ev) => writer
                .write_event(ev)
                .map_err(|e| ForgeError::Parse {
                    path: "workbook.xml".into(),
                    message: format!("xml write: {}", e),
                })?,
            Err(e) => {
                return Err(ForgeError::Parse {
                    path: "workbook.xml".into(),
                    message: format!("xml: {}", e),
                })
            }
        }
        buf.clear();
    }
    Ok(out)
}

fn is_sheet(e: &BytesStart) -> bool {
    e.name().as_ref() == b"sheet"
}

fn sheet_has_macro_rid(e: &BytesStart, macro_ids: &HashSet<String>) -> bool {
    for a in e.attributes().with_checks(false).flatten() {
        if a.key.as_ref() == b"r:id" {
            if let Ok(v) = a.unescape_value() {
                if macro_ids.contains(v.as_ref()) {
                    return true;
                }
            }
        }
    }
    false
}

/// Rewrite `xl/_rels/workbook.xml.rels`, dropping every `<Relationship>` whose
/// Id is in `macro_ids`.
fn strip_rels_entries(input: &[u8], macro_ids: &HashSet<String>) -> ForgeResult<Vec<u8>> {
    if macro_ids.is_empty() {
        return Ok(input.to_vec());
    }
    let mut reader = Reader::from_reader(input);
    reader.trim_text(false);
    let mut out = Vec::with_capacity(input.len());
    let mut writer = Writer::new(&mut out);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e))
                if e.name().as_ref() == b"Relationship" && rel_id_is_macro(e, macro_ids) =>
            {
                // Drop — empty event already consumed. For Start, content is trivial.
            }
            Ok(Event::Eof) => break,
            Ok(ev) => writer
                .write_event(ev)
                .map_err(|e| ForgeError::Parse {
                    path: "workbook.xml.rels".into(),
                    message: format!("xml write: {}", e),
                })?,
            Err(e) => {
                return Err(ForgeError::Parse {
                    path: "workbook.xml.rels".into(),
                    message: format!("xml: {}", e),
                })
            }
        }
        buf.clear();
    }
    Ok(out)
}

fn rel_id_is_macro(e: &BytesStart, macro_ids: &HashSet<String>) -> bool {
    for a in e.attributes().with_checks(false).flatten() {
        if a.key.as_ref() == b"Id" {
            if let Ok(v) = a.unescape_value() {
                return macro_ids.contains(v.as_ref());
            }
        }
    }
    false
}
