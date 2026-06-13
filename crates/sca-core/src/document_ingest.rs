#![cfg(feature = "docx")]
//! Document Brain Plugin — extract PDF/DOCX/TXT/MD into .said frames.
//!
//! Mirrors the `whisper_ingest` shape: one public `ingest_document` entry point,
//! a `DocSegment` carrier, and an `IngestReport`. Extraction is streamed —
//! each page/paragraph/chunk emits through the progress callback as it's
//! processed, so the CLI can render a live progress bar identical to
//! `said init` and `said ingest <video>`.
//!
//! **Per-format granularity** (one frame = one logical unit the user can cite):
//!   - PDF  → one frame per page       (doc_id: `<rel>::page_<N>`)
//!   - DOCX → one frame per paragraph  (doc_id: `<rel>::para_<N>`)
//!   - TXT  → one frame per 512-char chunk with 256-char stride
//!   - MD   → same chunking as TXT
//!
//! Every ingested frame carries:
//!   - `source:<abs_path>` tag (foundation for a future `said watch` daemon)
//!   - `ingest:doc_pdf` / `ingest:doc_docx` / `ingest:doc_text` kind tag
//!   - Format-specific location tags (`page:N`, `para:N`, `chunk:N`)
//!   - `blake3:<hash>` tag (for re-ingest skip)
//!
//! All formats live behind the `docs` feature flag (zero cost if not enabled).

use std::path::{Path, PathBuf};

use crate::frames::{MemoryKind, MemoryScope, MemorySubject, MemoryType, PutOptions};

// ────────────────────────────────────────────────────────────────────────────
// Auto-tagging: scan filename + content for metadata patterns
// ────────────────────────────────────────────────────────────────────────────

/// Detect metadata tags from a filename. Scans for common patterns:
///   - `v1`, `v2`, `_v4`, `-v10` → `version:N`
///   - `draft`, `DRAFT` → `status:draft`
///   - `final`, `FINAL` → `status:final`
///   - `confidential`, `CONFIDENTIAL` → `status:confidential`
///   - `internal` → `status:internal`
pub fn auto_tags_from_filename(filename: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let lower = filename.to_lowercase();

    // Version: v1, v2, _v4, -v10, version-1, version_2
    let chars: Vec<char> = lower.chars().collect();
    for i in 0..chars.len().saturating_sub(1) {
        if chars[i] == 'v' && chars[i + 1].is_ascii_digit() {
            let boundary = i == 0 || matches!(chars[i - 1], '_' | '-' | '.' | ' ');
            if boundary {
                let num: String = chars[i + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if !num.is_empty() {
                    tags.push(format!("version:{}", num));
                    break;
                }
            }
        }
    }

    // Status keywords
    if lower.contains("draft") { tags.push("status:draft".to_string()); }
    if lower.contains("final") { tags.push("status:final".to_string()); }
    if lower.contains("confidential") { tags.push("status:confidential".to_string()); }
    if lower.contains("internal") { tags.push("status:internal".to_string()); }

    tags
}

/// Detect metadata tags from the first segment's text content. Scans for:
///   - "Version N" / "Revision N" / "Rev. N" / "v N" → `version:N`
///   - "DRAFT" / "CONFIDENTIAL" / "INTERNAL" → `status:*`
///   - "Effective Date: YYYY-MM-DD" → `effective_date:YYYY-MM-DD`
pub fn auto_tags_from_content(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let lower = text.to_lowercase();

    // Version/Revision from content: "Version 4", "Revision 3.0", "Rev. 2"
    let words: Vec<&str> = lower.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        if (*w == "version" || *w == "revision" || *w == "rev." || *w == "rev")
            && i + 1 < words.len()
        {
            let next = words[i + 1].trim_end_matches(|c: char| !c.is_ascii_digit());
            let num: String = next.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !num.is_empty() {
                tags.push(format!("version:{}", num));
                break;
            }
        }
    }

    // Status from content
    if lower.contains("draft") { tags.push("status:draft".to_string()); }
    if text.contains("CONFIDENTIAL") { tags.push("status:confidential".to_string()); }
    if text.contains("INTERNAL") { tags.push("status:internal".to_string()); }

    tags
}

// ────────────────────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────────────────────

/// One logical unit extracted from a document (page, paragraph, or chunk).
#[derive(Debug, Clone)]
pub struct DocSegment {
    /// 1-based sequence number (page 1, paragraph 1, chunk 1, ...)
    pub index: usize,
    /// Extracted text for this segment (trimmed, non-empty)
    pub text: String,
    /// Human-readable location label ("page 3", "paragraph 42", "chunk 7")
    pub label: String,
}

/// Outcome of ingesting one document file.
#[derive(Debug, Clone)]
pub struct IngestReport {
    pub source_path: String,
    pub format: &'static str,
    pub segments_extracted: usize,
    pub frames_stored: usize,
    pub skipped: bool,
    pub elapsed_ms: u128,
}

impl IngestReport {
    /// Flat key/value pairs for the CLI to print. Mirrors whisper_ingest.
    pub fn as_pairs(&self) -> Vec<(String, String)> {
        vec![
            ("source_path".to_string(), self.source_path.clone()),
            ("format".to_string(), self.format.to_string()),
            ("segments".to_string(), self.segments_extracted.to_string()),
            ("frames_stored".to_string(), self.frames_stored.to_string()),
            ("skipped".to_string(), self.skipped.to_string()),
            ("elapsed_ms".to_string(), self.elapsed_ms.to_string()),
        ]
    }
}

/// Supported document formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFormat {
    Pdf,
    Docx,
    Text,
    Markdown,
}

impl DocFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "pdf" => Some(Self::Pdf),
            "docx" => Some(Self::Docx),
            "txt" => Some(Self::Text),
            "md" | "markdown" => Some(Self::Markdown),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Text => "txt",
            Self::Markdown => "md",
        }
    }

    pub fn ingest_tag(self) -> &'static str {
        match self {
            Self::Pdf => "ingest:doc_pdf",
            Self::Docx => "ingest:doc_docx",
            Self::Text => "ingest:doc_text",
            Self::Markdown => "ingest:doc_md",
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Format-specific extraction — each yields DocSegment through a closure so
// the caller can render live progress as pages come out.
// ────────────────────────────────────────────────────────────────────────────

/// Extract a PDF page-by-page. pdf-extract returns `Vec<String>` from
/// `extract_text_by_pages` — we hand each page to `on_segment` as soon as it
/// lands so progress ticks even on big PDFs.
#[cfg(feature = "docs")]
pub fn extract_pdf<F>(path: &Path, on_segment: F) -> Result<usize, String>
where
    F: FnMut(DocSegment),
{
    // Try pdfium first (layout-aware: handles multi-column, tables, reading order).
    // Falls back to pdf-extract (pure Rust) if pdfium library isn't available.
    match extract_pdf_pdfium(path) {
        Ok(raw_pages) => {
            eprintln!("[pdf] pdfium extracted {} pages", raw_pages.len());
            return extract_pdf_postprocess(raw_pages, path, on_segment);
        }
        Err(e) => {
            eprintln!("[pdf] pdfium unavailable ({}), falling back to pdf-extract", e);
        }
    }

    extract_pdf_fallback(path, on_segment)
}

/// Pdfium-based extraction: layout-aware, handles multi-column and complex layouts.
/// Uses Google's pdfium engine which reads text in correct column/reading order.
/// Requires the pdfium shared library to be installed (auto-detected at runtime).
#[cfg(feature = "docs")]
fn extract_pdf_pdfium(path: &Path) -> Result<Vec<String>, String> {
    let doc = pdfium::PdfiumDocument::new_from_path(
        path.to_str().ok_or_else(|| "non-UTF8 path".to_string())?,
        None,
    ).map_err(|e| format!("pdfium load: {:?}", e))?;

    let mut pages = Vec::new();
    for page_result in doc.pages() {
        let page = page_result.map_err(|e| format!("pdfium page: {:?}", e))?;
        let text = page.text().map_err(|e| format!("pdfium text: {:?}", e))?;
        pages.push(text.full());
    }
    Ok(pages)
}

/// Pure-Rust fallback when pdfium isn't available.
#[cfg(feature = "docs")]
fn extract_pdf_fallback<F>(path: &Path, on_segment: F) -> Result<usize, String>
where
    F: FnMut(DocSegment),
{
    let bytes = std::fs::read(path)
        .map_err(|e| format!("read pdf: {}", e))?;
    let raw_pages = pdf_extract::extract_text_from_mem_by_pages(&bytes)
        .map_err(|e| format!("pdf parse: {}", e))?;
    let pages: Vec<String> = raw_pages.into_iter().collect();
    extract_pdf_postprocess(pages, path, on_segment)
}

/// Common post-processing: sentence stitching + watermark stripping + OCR + emit segments.
#[cfg(feature = "docs")]
fn extract_pdf_postprocess<F>(raw_pages: Vec<String>, pdf_path: &Path, mut on_segment: F) -> Result<usize, String>
where
    F: FnMut(DocSegment),
{

    // Post-extraction pass: stitch orphaned sentences at page boundaries.
    // If page N ends mid-sentence (no terminal punctuation) and page N+1
    // starts with a continuation (lowercase letter or digit), the tail of
    // page N is prepended to page N+1 so the complete thought lives in one
    // frame. This fixes Chamber 1 ("measured at exactly" + "47.3 MHz").
    let mut pages: Vec<String> = raw_pages
        .iter()
        .map(|p| p.trim().to_string())
        .collect();

    for i in 0..pages.len().saturating_sub(1) {
        let ends_mid = !pages[i].is_empty() && {
            let last_char = pages[i].trim_end().chars().last().unwrap_or('.');
            !matches!(last_char, '.' | '!' | '?' | ':' | ';' | ')' | ']' | '"')
        };
        let next_continues = !pages[i + 1].is_empty() && {
            let first_char = pages[i + 1].trim_start().chars().next().unwrap_or('A');
            first_char.is_lowercase() || first_char.is_ascii_digit()
        };
        if ends_mid && next_continues {
            // Find the orphaned sentence fragment: last line(s) of page N that
            // don't end with terminal punctuation. Take from the last sentence
            // break backward.
            let tail = {
                let lines: Vec<&str> = pages[i].lines().collect();
                let mut start = lines.len();
                for j in (0..lines.len()).rev() {
                    let line = lines[j].trim();
                    if line.is_empty() { break; }
                    let lc = line.chars().last().unwrap_or('.');
                    if matches!(lc, '.' | '!' | '?' | ':' | ';') {
                        break;
                    }
                    start = j;
                }
                if start < lines.len() {
                    lines[start..].join(" ").trim().to_string()
                } else {
                    String::new()
                }
            };
            if !tail.is_empty() {
                // Prepend the orphaned tail to page N+1
                pages[i + 1] = format!("{} {}", tail, pages[i + 1]);
            }
        }
    }

    // Strip common headers/footers: if the same short string appears at the
    // start or end of ≥60% of pages, it's likely a repeated header/footer.
    // Remove it from every page to reduce noise in SCA fingerprints.
    if pages.len() >= 3 {
        // Check first line of each page for repeated headers
        let first_lines: Vec<String> = pages.iter()
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.lines().next().map(|l| l.trim().to_string()))
            .collect();
        if first_lines.len() >= 3 {
            let threshold = (first_lines.len() * 60) / 100;
            let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for line in &first_lines {
                if line.len() < 80 {
                    *counts.entry(line.clone()).or_insert(0) += 1;
                }
            }
            let headers_to_strip: Vec<String> = counts.into_iter()
                .filter(|(_, count)| *count >= threshold)
                .map(|(header, _)| header)
                .collect();
            for header in &headers_to_strip {
                for page in &mut pages {
                    if let Some(rest) = page.strip_prefix(header.as_str()) {
                        *page = rest.trim_start().to_string();
                    }
                }
            }
        }
    }

    // Strip watermarks: short standalone lines (≤3 words, all-caps or mixed)
    // that appear on ≥60% of non-empty pages are likely watermarks ("DRAFT",
    // "CONFIDENTIAL", "INTERNAL USE ONLY"). Remove them from all pages.
    if pages.len() >= 3 {
        let non_empty = pages.iter().filter(|p| !p.trim().is_empty()).count();
        let threshold = (non_empty * 60) / 100;
        // Collect candidate watermark lines (short, appear often)
        let mut line_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for page in &pages {
            // Deduplicate within a single page so a watermark repeated twice
            // on one page doesn't inflate the count.
            let mut seen_on_page: std::collections::HashSet<String> = std::collections::HashSet::new();
            for line in page.lines() {
                let trimmed = line.trim().to_string();
                let word_count = trimmed.split_whitespace().count();
                if word_count >= 1 && word_count <= 4 && trimmed.len() <= 40 {
                    if seen_on_page.insert(trimmed.clone()) {
                        *line_counts.entry(trimmed).or_insert(0) += 1;
                    }
                }
            }
        }
        let watermarks: Vec<String> = line_counts
            .into_iter()
            .filter(|(_, count)| *count >= threshold)
            .map(|(line, _)| line)
            .collect();
        if !watermarks.is_empty() {
            for page in &mut pages {
                let lines: Vec<&str> = page.lines().collect();
                let cleaned: Vec<&str> = lines
                    .into_iter()
                    .filter(|line| {
                        let trimmed = line.trim();
                        !watermarks.iter().any(|wm| trimmed == wm)
                    })
                    .collect();
                *page = cleaned.join("\n");
            }
        }
    }

    // OCR pass: for pages with zero text (image-only / scanned), render
    // the page to a bitmap via pdfium and run PaddleOCR. Only fires when
    // the `ocr` feature is enabled; otherwise image-only pages emit a
    // warning and are skipped.
    #[cfg(feature = "ocr")]
    {
        // Count image-only pages to decide if OCR is worth attempting.
        let image_only_pages: Vec<usize> = pages.iter().enumerate()
            .filter(|(_, t)| t.trim().is_empty())
            .map(|(i, _)| i)
            .collect();

        if !image_only_pages.is_empty() {
            // Open PDF once for all OCR pages. The entire render+OCR pipeline
            // runs in a background thread with a per-page timeout so a hung
            // render on a massive scanned page can't block the ingest.
            let pdf_path_str = pdf_path.to_str().unwrap_or("").to_string();
            let (tx, rx) = std::sync::mpsc::channel::<(usize, Result<String, String>)>();

            let pages_to_ocr = image_only_pages.clone();
            std::thread::spawn(move || {
                use pdfium::PdfiumRenderConfig;
                let doc = match pdfium::PdfiumDocument::new_from_path(&pdf_path_str, None) {
                    Ok(d) => d,
                    Err(e) => {
                        // Can't open for rendering — send error for all pages
                        for &pg in &pages_to_ocr {
                            let _ = tx.send((pg, Err(format!("pdfium render: {:?}", e))));
                        }
                        return;
                    }
                };
                for &pg in &pages_to_ocr {
                    let result = (|| -> Result<String, String> {
                        let page = doc.page(pg as i32)
                            .map_err(|e| format!("page {}: {:?}", pg + 1, e))?;
                        let render_width = (page.width() * 150.0 / 72.0) as i32;
                        let config = PdfiumRenderConfig::default().with_width(render_width);
                        let bitmap = page.render(&config)
                            .map_err(|e| format!("render: {:?}", e))?;
                        let img = bitmap.as_rgba8_image()
                            .map_err(|e| format!("bitmap: {:?}", e))?;
                        crate::ocr_ingest::recognize_dynamic_image(&img)
                    })();
                    if tx.send((pg, result)).is_err() {
                        break; // receiver dropped, stop
                    }
                }
            });

            // Collect results with a 30s timeout per page.
            for _ in &image_only_pages {
                match rx.recv_timeout(std::time::Duration::from_secs(30)) {
                    Ok((pg, Ok(text))) if !text.trim().is_empty() => {
                        eprintln!("[ocr] page {} — {} chars recognized", pg + 1, text.len());
                        pages[pg] = text;
                    }
                    Ok((pg, Ok(_))) => {
                        eprintln!("[ocr] page {} — no text detected", pg + 1);
                    }
                    Ok((pg, Err(e))) => {
                        eprintln!("[ocr] page {} — failed: {}", pg + 1, e);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        eprintln!("[ocr] timed out (30s) — skipping remaining OCR pages");
                        break; // don't wait for more pages, move on
                    }
                    Err(_) => {
                        eprintln!("[ocr] thread crashed — skipping remaining OCR pages");
                        break;
                    }
                }
            }
        }
    }
    #[cfg(not(feature = "ocr"))]
    {
        // Warn about image-only pages when OCR is not enabled
        for (i, page_text) in pages.iter().enumerate() {
            if page_text.trim().is_empty() {
                eprintln!("[pdf] page {} is image-only — enable --features ocr for text extraction", i + 1);
            }
        }
        let _ = pdf_path; // suppress unused warning
    }

    let mut emitted = 0usize;
    for (i, text) in pages.iter().enumerate() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let idx = i + 1;
        on_segment(DocSegment {
            index: idx,
            text: trimmed.to_string(),
            label: format!("page {}", idx),
        });
        emitted += 1;
    }
    Ok(emitted)
}

/// DOCX: unzip → read `word/document.xml` → walk `<w:p>` blocks →
/// concatenate their `<w:t>` text runs → emit one DocSegment per paragraph.
///
/// Minimal quick-xml state machine, ~40 lines. Matches memvid's docx.rs
/// approach but simpler (we don't need style/table extraction for v1).
pub fn extract_docx<F>(path: &Path, on_segment: F) -> Result<usize, String>
where
    F: FnMut(DocSegment),
{
    let bytes = std::fs::read(path)
        .map_err(|e| format!("open docx: {}", e))?;
    extract_docx_bytes(&bytes, on_segment)
}

/// DOCX (filesystem-free): same `<w:p>`/`<w:t>` walk as [`extract_docx`], but
/// the archive is opened from an in-memory byte slice via `Cursor` rather than
/// a file path. This is the WASM-safe core; `extract_docx` reads the file then
/// delegates here. Behaviour is identical.
pub fn extract_docx_bytes<F>(bytes: &[u8], mut on_segment: F) -> Result<usize, String>
where
    F: FnMut(DocSegment),
{
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("unzip docx: {}", e))?;
    let mut doc_xml_file = zip.by_name("word/document.xml")
        .map_err(|e| format!("docx missing word/document.xml: {}", e))?;
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut doc_xml_file, &mut xml)
        .map_err(|e| format!("read document.xml: {}", e))?;

    let mut reader = Reader::from_str(&xml);
    reader.trim_text(true);
    let mut buf = Vec::new();

    let mut in_paragraph = false;
    let mut in_text_run = false;
    let mut current = String::new();
    let mut paragraph_idx = 0usize;
    let mut emitted = 0usize;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(format!("docx parse at {}: {}", reader.buffer_position(), e)),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                match e.name().as_ref() {
                    b"w:p" => { in_paragraph = true; current.clear(); }
                    b"w:t" if in_paragraph => { in_text_run = true; }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                match e.name().as_ref() {
                    b"w:p" => {
                        in_paragraph = false;
                        let trimmed = current.trim();
                        if !trimmed.is_empty() {
                            paragraph_idx += 1;
                            on_segment(DocSegment {
                                index: paragraph_idx,
                                text: trimmed.to_string(),
                                label: format!("paragraph {}", paragraph_idx),
                            });
                            emitted += 1;
                        }
                        current.clear();
                    }
                    b"w:t" => { in_text_run = false; }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) if in_text_run => {
                if let Ok(s) = t.unescape() {
                    current.push_str(s.as_ref());
                    current.push(' ');
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(emitted)
}

/// TXT / MD: UTF-8 validation, then sliding-window chunking (512 chars, 256
/// stride). Mirrors the existing `chunk_text` in said_file.rs so retrieval
/// quality on plain text documents matches the rest of the pipeline.
pub fn extract_text<F>(path: &Path, mut on_segment: F) -> Result<usize, String>
where
    F: FnMut(DocSegment),
{
    let bytes = std::fs::read(path)
        .map_err(|e| format!("read text: {}", e))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| "file is not valid UTF-8 (BOM-aware decoding not in v1)".to_string())?;

    const CHUNK: usize = 512;
    const STRIDE: usize = 256;
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut start = 0;
    let mut idx = 0usize;
    let mut emitted = 0usize;

    while start < n {
        let end = (start + CHUNK).min(n);
        let chunk: String = chars[start..end].iter().collect();
        let trimmed = chunk.trim();
        if trimmed.len() >= 50 {
            idx += 1;
            on_segment(DocSegment {
                index: idx,
                text: trimmed.to_string(),
                label: format!("chunk {}", idx),
            });
            emitted += 1;
        }
        if end >= n { break; }
        start += STRIDE;
    }

    // Fallback: tiny files that never crossed the 50-char threshold — index
    // them whole so nothing is silently dropped.
    if emitted == 0 && !text.trim().is_empty() {
        on_segment(DocSegment {
            index: 1,
            text: text.trim().to_string(),
            label: "chunk 1".to_string(),
        });
        emitted = 1;
    }
    Ok(emitted)
}

// ────────────────────────────────────────────────────────────────────────────
// Public entry point — mirrors `whisper_ingest::ingest_video`
// ────────────────────────────────────────────────────────────────────────────

/// Ingest a single document file.
///
/// The `progress` callback fires once per extracted segment, giving the caller
/// (done, total_estimate, label). `total_estimate` is 0 when the format
/// doesn't expose a page count upfront — in that case the caller should
/// render spinner-style progress instead of a bar.
pub fn ingest_document<F>(
    brain: &mut crate::said_file::SaidFile,
    doc_path: &str,
    mut progress: F,
) -> Result<IngestReport, String>
where
    F: FnMut(usize, usize, &str),
{
    let t0 = crate::time_compat::Stopwatch::start();
    let path = Path::new(doc_path);
    if !path.is_file() {
        return Err(format!("not a file: {}", doc_path));
    }

    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let format = DocFormat::from_extension(ext)
        .ok_or_else(|| format!("unsupported document extension: .{}", ext))?;

    let abs: PathBuf = std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf());
    let abs_str = abs.to_string_lossy().replace('\\', "/");
    let source_tag = format!("source:{}", abs_str);

    // Dedupe by BLAKE3 content hash — re-ingest of the same file is a no-op.
    let file_bytes = std::fs::read(&abs)
        .map_err(|e| format!("read: {}", e))?;
    let hash = blake3::hash(&file_bytes);
    let hash_tag = format!("blake3:{}", hash.to_hex());

    let already = brain.frames.active_doc_ids().iter().any(|did| {
        brain.frames.get_meta(did)
            .map(|m| m.tags.iter().any(|t| t == &hash_tag))
            .unwrap_or(false)
    });
    if already {
        return Ok(IngestReport {
            source_path: doc_path.to_string(),
            format: format.label(),
            segments_extracted: 0,
            frames_stored: 0,
            skipped: true,
            elapsed_ms: (t0.elapsed_micros() / 1000) as u128,
        });
    }
    drop(file_bytes); // release before extractors re-read

    let filename = path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let rel_path = filename.clone();

    // Auto-detect metadata tags from the filename (version, status, etc.).
    // These propagate to EVERY frame from this file so tag-filtered scoring
    // can narrow the candidate pool before semantic ranking.
    let file_meta_tags = auto_tags_from_filename(&filename);

    let mut frames_stored = 0usize;
    let mut segments_extracted = 0usize;
    // Track whether we've scanned the first segment for content-based tags.
    let mut content_meta_tags: Option<Vec<String>> = None;

    let mut on_segment = |seg: DocSegment| {
        segments_extracted += 1;

        // On the first segment, also scan its text for metadata patterns
        // (catches "Version 4" in the document header when the filename
        // doesn't contain a version number).
        if content_meta_tags.is_none() {
            content_meta_tags = Some(auto_tags_from_content(&seg.text));
        }

        let doc_id = match format {
            DocFormat::Pdf  => format!("{}::page_{:04}", rel_path, seg.index),
            DocFormat::Docx => format!("{}::para_{:04}", rel_path, seg.index),
            DocFormat::Text | DocFormat::Markdown =>
                format!("{}::chunk_{:04}", rel_path, seg.index),
        };
        let title = format!("{} [{}]", filename, seg.label);
        let loc_tag = match format {
            DocFormat::Pdf  => format!("page:{}", seg.index),
            DocFormat::Docx => format!("para:{}", seg.index),
            DocFormat::Text | DocFormat::Markdown => format!("chunk:{}", seg.index),
        };
        let mut tags = vec![
            source_tag.clone(),
            format.ingest_tag().to_string(),
            loc_tag,
            hash_tag.clone(),
        ];
        // Append auto-detected metadata tags (version, status, etc.)
        tags.extend(file_meta_tags.iter().cloned());
        if let Some(ref ct) = content_meta_tags {
            for t in ct {
                if !tags.contains(t) {
                    tags.push(t.clone());
                }
            }
        }
        let opts = PutOptions::new(&doc_id, &seg.text)
            .with_title(&title)
            .with_type(MemoryType::Episodic)
            .with_kind(MemoryKind::Fact)
            .with_subject(MemorySubject::World)
            .with_scope(MemoryScope::Personal)
            .with_tags(tags);
        brain.put_with(&opts);
        frames_stored += 1;

        progress(segments_extracted, 0, &seg.label);
    };

    match format {
        #[cfg(feature = "docs")]
        DocFormat::Pdf => { extract_pdf(&abs, &mut on_segment)?; }
        #[cfg(not(feature = "docs"))]
        DocFormat::Pdf => {
            return Err("PDF extraction requires the `docs` feature (native pdfium); \
                        this build only has DOCX/TXT/MD support".to_string());
        }
        DocFormat::Docx => { extract_docx(&abs, &mut on_segment)?; }
        DocFormat::Text | DocFormat::Markdown => { extract_text(&abs, &mut on_segment)?; }
    }

    Ok(IngestReport {
        source_path: doc_path.to_string(),
        format: format.label(),
        segments_extracted,
        frames_stored,
        skipped: false,
        elapsed_ms: (t0.elapsed_micros() / 1000) as u128,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_docx_bytes_matches_paragraph_count() {
        use std::io::Write;
        let buf = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(buf);
        let opts = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let doc = r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>one</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r></w:p><w:p><w:r><w:t>three</w:t></w:r></w:p></w:body></w:document>"#;
        zw.start_file("word/document.xml", opts).unwrap();
        zw.write_all(doc.as_bytes()).unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        let mut segs: Vec<String> = Vec::new();
        let n = extract_docx_bytes(&bytes, |s| segs.push(s.text)).unwrap();
        assert_eq!(n, 3);
        assert_eq!(
            segs,
            vec!["one".to_string(), "two".to_string(), "three".to_string()]
        );
    }
}
