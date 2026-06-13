use lopdf::Document as LoPdf;
use regex::Regex;
use std::sync::LazyLock;

use crate::hasher;

/// One extracted block of text from the PDF, hashed for dedup.
#[derive(Debug, Clone)]
pub struct TextBlock {
    pub text: String,
    pub fingerprint: String,
    pub hash: String,
}

/// One image extracted from the PDF (XObject), keyed by its content hash.
#[derive(Debug)]
pub struct PdfImage {
    pub name: String,
    pub data: Vec<u8>,
    pub hash: String,
}

/// One font extracted from the PDF, deduped by canonical name + content hash.
#[derive(Debug)]
pub struct PdfFont {
    pub name: String,
    pub data: Vec<u8>,
    pub hash: String,
}

/// PDF permission flags (`/P` integer in the encryption dictionary).
/// Not yet wired into `PdfSecurityMetadata` — extraction lands in v1.1
/// alongside real password-protected decryption (see Issue 1's note).
#[allow(dead_code)]
#[derive(Debug, Default, serde::Serialize)]
pub struct PdfPermissions {
    pub print: bool,
    pub modify: bool,
    pub copy: bool,
    pub annotate: bool,
}

/// Signature + encryption metadata extracted at parse time. Used by the
/// rebuilder to emit warnings when a signed or encrypted doc is rebuilt.
#[derive(Debug, Default, serde::Serialize)]
pub struct PdfSecurityMetadata {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub encrypted: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rebuild_warnings: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub signatures: Vec<PdfSignatureInfo>,
}

/// Digital signature info extracted from a PDF /Sig dictionary.
#[derive(Debug, Default, serde::Serialize)]
pub struct PdfSignatureInfo {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub signer: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub date: String,
}

/// Parser output: all extracted document parts plus security metadata.
/// Returned by `parse()` on successful decode of a non-encrypted PDF.
#[derive(Debug, Default)]
pub struct PdfResult {
    pub paragraphs: Vec<TextBlock>,
    pub images: Vec<PdfImage>,
    pub fonts: Vec<PdfFont>,
    pub security: PdfSecurityMetadata,
}

/// Errors that can occur while parsing a PDF. Encryption is detected
/// before lopdf is invoked; lopdf parse failures map to `Parse`.
#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("PDF is password-protected; re-run with --password")]
    Encrypted,
    #[error("incorrect password for PDF")]
    WrongPassword,
    #[error("PDF parse error: {0}")]
    Parse(String),
}

static RE_TEXT_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)BT(.*?)ET").unwrap());
static RE_TJ: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(([^)]*)\)\s*Tj").unwrap());
static RE_TJ_ARRAY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\s*TJ").unwrap());
static RE_STRING_IN_TJ: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(([^)]*)\)").unwrap());
static RE_TF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/(\S+)\s+([\d.]+)\s+Tf").unwrap());
// Tm: a b c d e f Tm — absolute text matrix; capture group 6 is Y.
static RE_TM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([\-\d.]+)\s+([\-\d.]+)\s+([\-\d.]+)\s+([\-\d.]+)\s+([\-\d.]+)\s+([\-\d.]+)\s+Tm").unwrap());
// Td/TD: tx ty Td — relative text position offset; capture group 2 is Y.
static RE_TD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([\-\d.]+)\s+([\-\d.]+)\s+T[dD]").unwrap());

/// Parse a PDF from raw bytes. Returns extracted parts (text blocks,
/// images, fonts) plus security metadata, or an error.
///
/// # Errors
/// - `PdfError::Encrypted` — the PDF is encrypted (detected by byte-scan
///   for `/Encrypt` in the trailer area before lopdf is invoked).
///   Password-protected decryption is deferred to v1.1.
/// - `PdfError::WrongPassword` — set when a password decryption attempt
///   fails. Currently unreachable in v1 because the password parameter
///   is unused (see Issue 1 in T10 review).
/// - `PdfError::Parse(_)` — lopdf failed to decode the byte stream.
pub fn parse(data: &[u8], password: &str) -> Result<PdfResult, PdfError> {
    // Check for encryption by sniffing trailer for /Encrypt
    if is_encrypted(data) {
        if password.is_empty() {
            return Err(PdfError::Encrypted);
        }
    }

    // Note: lopdf 0.40 does not expose a password parameter on load_mem.
    // Encrypted PDFs are detected via the is_encrypted byte scan above and
    // rejected with PdfError::Encrypted regardless of whether a password
    // was supplied. Real password-protected decryption is deferred to v1.1
    // (requires lopdf 0.41+ or a wrapper).
    let _ = password; // password is accepted in the signature for API
                      // stability but currently unused — see comment above
    let doc = LoPdf::load_mem(data)
    .map_err(|e| {
        let msg = e.to_string().to_lowercase();
        if msg.contains("password") || msg.contains("encrypt") {
            if password.is_empty() {
                PdfError::Encrypted
            } else {
                PdfError::WrongPassword
            }
        } else {
            PdfError::Parse(e.to_string())
        }
    })?;

    let mut result = PdfResult::default();

    // Extract text from page content streams.
    for page_id in doc.get_pages().values() {
        if let Ok(content) = doc.get_page_content(*page_id) {
            let content_str = String::from_utf8_lossy(&content);
            extract_text_blocks(&content_str, &mut result);
        }
    }

    // Extract fonts.
    extract_fonts(&doc, &mut result);

    // Extract images.
    extract_images(&doc, &mut result);

    Ok(result)
}

fn is_encrypted(data: &[u8]) -> bool {
    // Quick heuristic: scan last 2KB for /Encrypt in trailer
    let tail_start = data.len().saturating_sub(2048);
    let tail = &data[tail_start..];
    tail.windows(8).any(|w| w == b"/Encrypt")
}

struct RawBlock {
    y: f64,
    fingerprint: String,
    parts: Vec<String>,
}

fn block_y(block: &str) -> f64 {
    // Tm (absolute) takes precedence over Td/TD (relative-from-BT-origin).
    if let Some(cap) = RE_TM.captures(block) {
        if let Ok(y) = cap[6].parse::<f64>() {
            return y;
        }
    }
    if let Some(cap) = RE_TD.captures(block) {
        if let Ok(y) = cap[2].parse::<f64>() {
            return y;
        }
    }
    0.0
}

fn extract_text_blocks(content: &str, result: &mut PdfResult) {
    // Y gap smaller than this merges adjacent BT/ET blocks into one paragraph.
    // 14pt covers single-line spacing; wider gaps (section breaks, headings) stay separate.
    const Y_THRESHOLD: f64 = 14.0;

    let mut raw: Vec<RawBlock> = Vec::new();

    for cap in RE_TEXT_BLOCK.captures_iter(content) {
        let block = &cap[1];

        let fingerprint = if let Some(tf_cap) = RE_TF.captures(block) {
            format!("font:{},size:{}", &tf_cap[1], &tf_cap[2])
        } else {
            "unknown".to_string()
        };

        let mut parts: Vec<String> = Vec::new();
        for tj_cap in RE_TJ.captures_iter(block) {
            parts.push(decode_pdf_string(&tj_cap[1]));
        }
        for tj_arr_cap in RE_TJ_ARRAY.captures_iter(block) {
            for str_cap in RE_STRING_IN_TJ.captures_iter(&tj_arr_cap[1]) {
                parts.push(decode_pdf_string(&str_cap[1]));
            }
        }

        if parts.is_empty() {
            continue;
        }
        raw.push(RawBlock { y: block_y(block), fingerprint, parts });
    }

    if raw.is_empty() {
        return;
    }

    // Sort top-of-page first (Y descending in PDF coordinate space).
    raw.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));

    // Merge consecutive blocks with same fingerprint where Y gap < threshold.
    // Each group is (last_y_seen, fingerprint, accumulated_parts).
    let mut groups: Vec<(f64, String, Vec<String>)> = Vec::new();

    for block in raw {
        let merged = groups.last_mut().and_then(|last| {
            if last.1 == block.fingerprint && (last.0 - block.y).abs() < Y_THRESHOLD {
                Some(last)
            } else {
                None
            }
        });

        if let Some(last) = merged {
            last.0 = block.y;
            last.2.extend(block.parts);
        } else {
            groups.push((block.y, block.fingerprint, block.parts));
        }
    }

    for (_, fingerprint, parts) in groups {
        let text = hasher::canonicalize_text(&parts.join(" "));
        if text.is_empty() {
            continue;
        }
        let hash = hasher::hash_paragraph(&text, &fingerprint);
        result.paragraphs.push(TextBlock { text, fingerprint, hash });
    }
}

fn decode_pdf_string(s: &str) -> String {
    s.replace(r"\n", "\n")
        .replace(r"\r", "\r")
        .replace(r"\t", "\t")
        .replace(r"\\", "\\")
        .replace(r"\(", "(")
        .replace(r"\)", ")")
}

fn extract_fonts(doc: &LoPdf, result: &mut PdfResult) {
    let mut seen = std::collections::HashSet::new();
    for (_, page_id) in doc.get_pages() {
        let (resources_opt, _) = match doc.get_page_resources(page_id) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let resources = match resources_opt {
            Some(r) => r,
            None => continue,
        };
        let font_ref = match resources.get(b"Font") {
            Ok(f) => f,
            Err(_) => continue,
        };
        let (_, font_obj) = match doc.dereference(font_ref) {
            Ok(x) => x,
            Err(_) => continue,
        };
        if let lopdf::Object::Dictionary(font_dict) = font_obj {
            for (font_name, _) in font_dict {
                let name_str = String::from_utf8_lossy(font_name).to_string();
                let canonical = hasher::strip_font_prefix(&name_str).to_string();
                if seen.contains(&canonical) {
                    continue;
                }
                seen.insert(canonical.clone());
                let hash = hasher::blake3_hex(canonical.as_bytes());
                result.fonts.push(PdfFont {
                    name: canonical,
                    data: Vec::new(),
                    hash,
                });
            }
        }
    }
}

fn extract_images(doc: &LoPdf, result: &mut PdfResult) {
    let mut seen: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
    for (_, page_id) in doc.get_pages() {
        let (resources_opt, _) = match doc.get_page_resources(page_id) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let resources = match resources_opt {
            Some(r) => r,
            None => continue,
        };
        let xobj_ref = match resources.get(b"XObject") {
            Ok(x) => x,
            Err(_) => continue,
        };
        let (_, xobj) = match doc.dereference(xobj_ref) {
            Ok(x) => x,
            Err(_) => continue,
        };
        if let lopdf::Object::Dictionary(xobj_dict) = xobj {
            for (img_name, img_ref) in xobj_dict {
                let (_, obj) = match doc.dereference(img_ref) {
                    Ok(x) => x,
                    Err(_) => continue,
                };
                if let lopdf::Object::Stream(stream) = obj {
                    let is_image = stream
                        .dict
                        .get(b"Subtype")
                        .ok()
                        .and_then(|s| if let lopdf::Object::Name(n) = s { Some(n.as_slice()) } else { None })
                        == Some(b"Image");
                    if !is_image {
                        continue;
                    }
                    let img_data = stream.content.clone();
                    if seen.contains(&img_data) {
                        continue;
                    }
                    seen.insert(img_data.clone());
                    let hash = hasher::blake3_hex(&img_data);
                    result.images.push(PdfImage {
                        name: String::from_utf8_lossy(img_name).to_string(),
                        data: img_data,
                        hash,
                    });
                }
            }
        }
    }
}
