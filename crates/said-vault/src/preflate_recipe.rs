//! Byte-exact DOCX recipe codec for the legal tier. `encode` turns a zip
//! (DOCX) into a PFR1 segment recipe — framing verbatim, each deflate stream
//! preflated (or verbatim on per-stream failure). `decode` reconstructs the
//! exact original bytes. Returns None from `encode` only for non-zip input.

use std::io::Cursor;
use preflate_rs::{preflate_whole_deflate_stream, recreate_whole_deflate_stream, PreflateConfig};

const MAGIC: &[u8; 4] = b"PFR1";
const TAG_VERBATIM: u8 = 0x00;
const TAG_PREFLATE: u8 = 0x01;

/// Encode a file's bytes into a byte-exact-reconstructable PFR1 recipe.
/// Returns None only when the input is not a valid zip (caller falls back to
/// whole-file verbatim). A single deflate stream that fails to round-trip is
/// emitted as a VERBATIM segment — it does not abort encoding.
pub fn encode(file_bytes: &[u8]) -> Option<Vec<u8>> {
    let mut arch = zip::ZipArchive::new(Cursor::new(file_bytes)).ok()?;

    let mut ranges: Vec<(usize, usize, bool)> = Vec::new();
    for i in 0..arch.len() {
        let e = arch.by_index_raw(i).ok()?;
        let start = e.data_start() as usize;
        let size = e.compressed_size() as usize;
        let is_deflate = e.compression() == zip::CompressionMethod::Deflated;
        ranges.push((start, start + size, is_deflate));
    }
    ranges.sort_by_key(|r| r.0);

    let cfg = PreflateConfig::default();
    let mut segments: Vec<Vec<u8>> = Vec::new();
    let mut cursor = 0usize;

    let push_verbatim = |segs: &mut Vec<Vec<u8>>, data: &[u8]| {
        let mut s = Vec::with_capacity(5 + data.len());
        s.push(TAG_VERBATIM);
        s.extend_from_slice(&(data.len() as u32).to_le_bytes());
        s.extend_from_slice(data);
        segs.push(s);
    };

    // ranges are sorted by start and, coming from a valid zip's distinct
    // non-overlapping entries, never overlap — so cursor advances monotonically
    // and each framing gap is filled exactly once.
    for (start, end, is_deflate) in &ranges {
        if *start > cursor {
            push_verbatim(&mut segments, &file_bytes[cursor..*start]);
        }
        let data = &file_bytes[*start..*end];
        let mut used_preflate = false;
        if *is_deflate {
            if let Ok((res, plain)) = preflate_whole_deflate_stream(data, &cfg) {
                let pt = plain.text();
                if let Ok(rebuilt) = recreate_whole_deflate_stream(pt, &res.corrections) {
                    if rebuilt == data {
                        let mut s = Vec::with_capacity(9 + pt.len() + res.corrections.len());
                        s.push(TAG_PREFLATE);
                        s.extend_from_slice(&(pt.len() as u32).to_le_bytes());
                        s.extend_from_slice(pt);
                        s.extend_from_slice(&(res.corrections.len() as u32).to_le_bytes());
                        s.extend_from_slice(&res.corrections);
                        segments.push(s);
                        used_preflate = true;
                    }
                }
            }
        }
        if !used_preflate {
            push_verbatim(&mut segments, data);
        }
        cursor = *end;
    }
    if cursor < file_bytes.len() {
        push_verbatim(&mut segments, &file_bytes[cursor..]);
    }

    let mut recipe = Vec::new();
    recipe.extend_from_slice(MAGIC);
    recipe.extend_from_slice(&(segments.len() as u32).to_le_bytes());
    for s in &segments {
        recipe.extend_from_slice(s);
    }
    Some(recipe)
}

/// Decode a PFR1 recipe back into the exact original file bytes.
pub fn decode(recipe: &[u8]) -> Result<Vec<u8>, String> {
    if recipe.len() < 8 {
        return Err("recipe too short for header".into());
    }
    if &recipe[0..4] != MAGIC {
        return Err(format!("bad recipe magic: {:?}", &recipe[0..4]));
    }
    let seg_count = u32::from_le_bytes(recipe[4..8].try_into().unwrap()) as usize;
    let mut out = Vec::new();
    let mut pos = 8;
    for i in 0..seg_count {
        if pos >= recipe.len() {
            return Err(format!("recipe truncated at segment {}", i));
        }
        let tag = recipe[pos];
        pos += 1;
        match tag {
            TAG_VERBATIM => {
                let len = read_u32(recipe, &mut pos, i, "verbatim len")?;
                let end = pos.checked_add(len).filter(|&e| e <= recipe.len())
                    .ok_or_else(|| format!("verbatim segment {} truncated", i))?;
                out.extend_from_slice(&recipe[pos..end]);
                pos = end;
            }
            TAG_PREFLATE => {
                let plain_len = read_u32(recipe, &mut pos, i, "plain len")?;
                let plain_end = pos.checked_add(plain_len).filter(|&e| e <= recipe.len())
                    .ok_or_else(|| format!("preflate segment {} plain truncated", i))?;
                let plain = &recipe[pos..plain_end];
                pos = plain_end;
                let corr_len = read_u32(recipe, &mut pos, i, "corr len")?;
                let corr_end = pos.checked_add(corr_len).filter(|&e| e <= recipe.len())
                    .ok_or_else(|| format!("preflate segment {} corr truncated", i))?;
                let corr = &recipe[pos..corr_end];
                pos = corr_end;
                let bytes = recreate_whole_deflate_stream(plain, corr)
                    .map_err(|e| format!("recreate segment {}: {:?}", i, e))?;
                out.extend_from_slice(&bytes);
            }
            other => return Err(format!("unknown segment tag {} at segment {}", other, i)),
        }
    }
    Ok(out)
}

/// Read a little-endian u32 at *pos, advancing *pos by 4. Bounds-checked.
fn read_u32(buf: &[u8], pos: &mut usize, seg: usize, what: &str) -> Result<usize, String> {
    if *pos > buf.len().saturating_sub(4) {
        return Err(format!("recipe truncated reading {} at segment {}", what, seg));
    }
    let v = u32::from_le_bytes(buf[*pos..*pos + 4].try_into().unwrap()) as usize;
    *pos += 4;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn build_docx(paragraph: &str) -> Vec<u8> {
        let buf = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(buf);
        let opts = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let files = [
            ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#),
            ("_rels/.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#),
        ];
        for (name, content) in &files {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(content.as_bytes()).unwrap();
        }
        let body: String = (0..30)
            .map(|i| format!("<w:p><w:r><w:t>{} {}</w:t></w:r></w:p>", paragraph, i))
            .collect();
        let doc = format!(r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{}</w:body></w:document>"#, body);
        zw.start_file("word/document.xml", opts).unwrap();
        zw.write_all(doc.as_bytes()).unwrap();
        zw.finish().unwrap().into_inner()
    }

    #[test]
    fn encode_then_decode_is_byte_exact_for_docx() {
        let docx = build_docx("byte exact me");
        let recipe = encode(&docx).expect("encode a valid docx");
        let restored = decode(&recipe).expect("decode");
        assert_eq!(restored, docx, "recipe must reconstruct the docx byte-for-byte");
    }

    #[test]
    fn encode_returns_none_for_non_zip() {
        let not_a_zip = b"this is plainly not a zip file".to_vec();
        assert!(encode(&not_a_zip).is_none());
    }

    #[test]
    fn encode_handles_stored_entries_byte_exact() {
        let buf = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(buf);
        let stored = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zw.start_file("stored.bin", stored).unwrap();
        zw.write_all(&[1u8, 2, 3, 4, 5]).unwrap();
        let defl = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zw.start_file("text.xml", defl).unwrap();
        zw.write_all(b"<x>hello hello hello hello</x>").unwrap();
        let zip_bytes = zw.finish().unwrap().into_inner();
        let recipe = encode(&zip_bytes).expect("encode");
        assert_eq!(decode(&recipe).unwrap(), zip_bytes);
    }

    #[test]
    fn decode_concatenates_verbatim_segments() {
        let mut r = Vec::new();
        r.extend_from_slice(MAGIC);
        r.extend_from_slice(&2u32.to_le_bytes());
        r.push(TAG_VERBATIM);
        r.extend_from_slice(&5u32.to_le_bytes());
        r.extend_from_slice(b"hello");
        r.push(TAG_VERBATIM);
        r.extend_from_slice(&6u32.to_le_bytes());
        r.extend_from_slice(b" world");
        let out = decode(&r).expect("decode");
        assert_eq!(out, b"hello world");
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let bad = b"XXXX\x00\x00\x00\x00".to_vec();
        assert!(decode(&bad).is_err());
    }

    #[test]
    fn decode_rejects_truncated() {
        let mut r = Vec::new();
        r.extend_from_slice(MAGIC);
        r.extend_from_slice(&1u32.to_le_bytes());
        r.push(TAG_VERBATIM);
        r.extend_from_slice(&100u32.to_le_bytes());
        assert!(decode(&r).is_err());
    }
}
