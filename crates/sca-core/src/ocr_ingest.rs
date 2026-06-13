#![cfg(feature = "ocr")]
//! OCR Plugin — PaddleOCR v5 via MNN inference for scanned/image-only PDFs.
//!
//! Models bundled at compile time via `include_bytes!()`. Self-contained.
//! Feature-gated: `#[cfg(feature = "ocr")]`.

use std::sync::OnceLock;

static DET_MODEL: &[u8] = include_bytes!("../../../ocr-models/PP-OCRv5_mobile_det_fp16.mnn");
static REC_MODEL: &[u8] = include_bytes!("../../../ocr-models/en_PP-OCRv5_mobile_rec_infer.mnn");
static KEYS_DATA: &[u8] = include_bytes!("../../../ocr-models/ppocr_keys_en.txt");

static OCR_ENGINE: OnceLock<Result<ocr_rs::OcrEngine, String>> = OnceLock::new();

fn get_engine() -> Result<&'static ocr_rs::OcrEngine, String> {
    OCR_ENGINE
        .get_or_init(|| {
            ocr_rs::OcrEngine::from_bytes(DET_MODEL, REC_MODEL, KEYS_DATA, None)
                .map_err(|e| format!("OCR engine init: {:?}", e))
        })
        .as_ref()
        .map_err(|e| e.clone())
}

/// Minimum confidence threshold for OCR results. Regions below this are
/// dropped to prevent garbage text from polluting SCA fingerprints.
/// PaddleOCR confidence is 0.0–1.0; 0.80 is conservative — keeps clean
/// text, drops anything the model is uncertain about. Raise to 0.90 for
/// maximum cleanliness at the cost of losing some marginal text.
const OCR_CONFIDENCE_THRESHOLD: f32 = 0.80;

/// Run OCR on a DynamicImage. Returns recognized text, one line per region,
/// sorted top to bottom. Low-confidence regions are dropped and garbage
/// patterns are cleaned before returning.
pub fn recognize_dynamic_image(
    image: &image::DynamicImage,
) -> Result<String, String> {
    let engine = get_engine()?;
    let results = engine
        .recognize(image)
        .map_err(|e| format!("OCR recognize: {:?}", e))?;

    // Filter by confidence + clean, then sort by Y position
    let mut lines: Vec<(f32, String)> = results
        .into_iter()
        .filter(|r| r.confidence >= OCR_CONFIDENCE_THRESHOLD)
        .map(|r| {
            let y = r.bbox.rect.top() as f32;
            let cleaned = clean_ocr_text(&r.text);
            (y, cleaned)
        })
        .filter(|(_, text)| !text.is_empty())
        .collect();
    lines.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    Ok(lines.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("\n"))
}

/// Clean common OCR garbage patterns from a recognized text line.
///
/// Drops:
///   - Lines shorter than 2 chars (isolated noise)
///   - Lines that are mostly non-alphanumeric (>50% special chars)
///   - Runs of the same character repeated 3+ times ("aaaa", "||||")
///   - Common OCR artifacts: isolated punctuation, repeated spaces
fn clean_ocr_text(raw: &str) -> String {
    let trimmed = raw.trim();

    // Too short — likely noise
    if trimmed.len() < 2 {
        return String::new();
    }

    // Mostly non-alphanumeric — likely garbage
    let alnum_count = trimmed.chars().filter(|c| c.is_alphanumeric()).count();
    let total_count = trimmed.chars().count();
    if total_count > 0 && (alnum_count as f32 / total_count as f32) < 0.50 {
        return String::new();
    }

    // Strip runs of repeated characters (3+ same char in a row)
    let mut cleaned = String::with_capacity(trimmed.len());
    let mut prev = '\0';
    let mut count = 0;
    for ch in trimmed.chars() {
        if ch == prev {
            count += 1;
            if count < 3 {
                cleaned.push(ch);
            }
        } else {
            prev = ch;
            count = 1;
            cleaned.push(ch);
        }
    }

    // Collapse multiple spaces
    let mut result = String::with_capacity(cleaned.len());
    let mut last_space = false;
    for ch in cleaned.chars() {
        if ch == ' ' {
            if !last_space {
                result.push(' ');
            }
            last_space = true;
        } else {
            result.push(ch);
            last_space = false;
        }
    }

    result.trim().to_string()
}

pub fn is_available() -> bool {
    get_engine().is_ok()
}
