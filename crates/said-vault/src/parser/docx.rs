use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashSet;
use std::io::Read;

use crate::hasher;

#[derive(Debug, Clone)]
pub struct Paragraph {
    pub text: String,
    pub fingerprint: String,
    pub hash: String,
}

#[derive(Debug)]
pub struct Image {
    pub name: String,
    pub data: Vec<u8>,
    pub hash: String,
}

#[derive(Debug)]
pub struct Font {
    pub name: String,
    pub data: Vec<u8>,
    pub hash: String,
}

#[derive(Debug)]
pub struct StructuralXml {
    pub name: String,
    pub data: Vec<u8>,
    pub hash: String,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct DocxSecurityMetadata {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub signed: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rebuild_warnings: Vec<String>,
}

#[derive(Debug, Default)]
pub struct DocxResult {
    pub paragraphs: Vec<Paragraph>,
    pub images: Vec<Image>,
    pub fonts: Vec<Font>,
    pub structural_xml: Vec<StructuralXml>,
    pub security: DocxSecurityMetadata,
}

pub fn parse(data: &[u8]) -> Result<DocxResult, String> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;

    let mut result = DocxResult::default();

    let names: Vec<String> = archive.file_names().map(String::from).collect();

    for name in &names {
        let mut entry = archive
            .by_name(name)
            .map_err(|e| format!("open entry {}: {}", name, e))?;
        let mut content = Vec::new();
        entry.read_to_end(&mut content).map_err(|e| e.to_string())?;
        drop(entry);

        if name == "word/document.xml" {
            // Extract paragraphs as the SEARCHABLE VIEW (powers `said ask`),
            // AND preserve the original bytes so the rebuilder can re-emit
            // them verbatim. Tables, lists, sections, sectPr, run-level
            // formatting (font/color/size) — none of that survives a parse-
            // and-regenerate round-trip, so we keep the source of truth.
            let paras = extract_paragraphs(&content)?;
            result.paragraphs = paras;
            let hash = hasher::blake3_hex(&content);
            result.structural_xml.push(StructuralXml {
                name: name.clone(),
                data: content,
                hash,
            });
        } else if name.starts_with("word/media/") {
            let hash = hasher::blake3_hex(&content);
            result.images.push(Image {
                name: name.clone(),
                data: content.clone(),
                hash,
            });
        } else if name.starts_with("word/fonts/") {
            let hash = hasher::blake3_hex(&content);
            result.fonts.push(Font {
                name: name.clone(),
                data: content.clone(),
                hash,
            });
        } else {
            // Preserve EVERY other zip entry by default. The fidelity audit
            // proved a hardcoded whitelist drops critical metadata in 100% of
            // real-world docs (docProps, numbering, headers/footers, fontTable,
            // customXml). Architectural inversion: extract what we know how to
            // (paragraphs/images/fonts above); preserve everything else verbatim
            // so rebuild can re-embed it.
            //
            // SEARCH gap fix: header/footer parts (word/headerN.xml, word/footerN.xml) share
            // the body's <w:p>/<w:t> grammar and hold the most DISCRIMINATING text in legal/
            // business docs — case numbers, dates, "RE:" / "OUR REF:" lines, registry refs.
            // Restore was already 1:1 (bytes preserved below), but `said ask` on a vault never
            // SAW that text because only document.xml fed the searchable view. Now we also
            // extract header/footer paragraphs INTO the searchable view (bytes still preserved
            // verbatim for 1:1 rebuild — this only ADDS to the search index, never changes
            // restore). So a vaulted legal doc is both byte-exact recoverable AND findable by
            // its header/footer reference.
            let is_header_footer = (name.starts_with("word/header") || name.starts_with("word/footer"))
                && name.ends_with(".xml");
            if is_header_footer {
                if let Ok(mut paras) = extract_paragraphs(&content) {
                    result.paragraphs.append(&mut paras);
                }
            }
            let hash = hasher::blake3_hex(&content);
            result.structural_xml.push(StructuralXml {
                name: name.clone(),
                data: content.clone(),
                hash,
            });
            if name.starts_with("_xmlsignatures/") && !result.security.signed {
                result.security.signed = true;
                result.security.rebuild_warnings.push(
                    "document contained digital signatures — signatures are re-embedded but \
                     cryptographically invalid; original signer info visible in Word".to_string(),
                );
            }
        }
    }

    Ok(result)
}

fn extract_paragraphs(data: &[u8]) -> Result<Vec<Paragraph>, String> {
    let mut reader = Reader::from_reader(data);
    reader.trim_text(true);

    let ns = b"http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    let mut paragraphs = Vec::new();
    let mut current_text = String::new();
    let mut current_props: HashSet<String> = HashSet::new();
    let mut in_paragraph = false;
    let mut in_run = false;
    let mut in_run_props = false;
    let mut in_t = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"p" => {
                        in_paragraph = true;
                        current_text.clear();
                        current_props.clear();
                    }
                    b"r" if in_paragraph => {
                        in_run = true;
                    }
                    b"rPr" if in_run => {
                        in_run_props = true;
                    }
                    b"b" if in_run_props => {
                        current_props.insert("bold".into());
                    }
                    b"i" if in_run_props => {
                        current_props.insert("italic".into());
                    }
                    b"t" if in_run => {
                        in_t = true;
                    }
                    _ => {}
                }
                let _ = ns; // future: namespace-aware parsing
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"b" if in_run_props => {
                        current_props.insert("bold".into());
                    }
                    b"i" if in_run_props => {
                        current_props.insert("italic".into());
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_t {
                    let raw = String::from_utf8_lossy(e.as_ref()).to_string();
                    current_text.push_str(&raw);
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"p" if in_paragraph => {
                        let normalized = hasher::canonicalize_text(&current_text);
                        if !normalized.is_empty() {
                            let mut props: Vec<String> =
                                current_props.iter().cloned().collect();
                            props.sort();
                            let fingerprint = if props.is_empty() {
                                "plain".to_string()
                            } else {
                                props.join(",")
                            };
                            let hash = hasher::hash_paragraph(&normalized, &fingerprint);
                            paragraphs.push(Paragraph {
                                text: normalized,
                                fingerprint,
                                hash,
                            });
                        }
                        in_paragraph = false;
                        in_run = false;
                    }
                    b"r" => {
                        in_run = false;
                        in_run_props = false;
                        in_t = false;
                    }
                    b"rPr" => {
                        in_run_props = false;
                    }
                    b"t" => {
                        in_t = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("parse docx xml: {}", e)),
            _ => {}
        }
        buf.clear();
    }

    Ok(paragraphs)
}
