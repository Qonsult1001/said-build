use std::io::Write;

pub fn rebuild(refs: &[crate::rebuild::RebuildRef]) -> Result<Vec<u8>, String> {
    let mut paragraph_xml_parts: Vec<String> = Vec::new();
    let mut written_files: std::collections::HashSet<String> = std::collections::HashSet::new();

    let buf = std::io::Cursor::new(Vec::new());
    let mut zw = zip::ZipWriter::new(buf);
    let opts = zip::write::FileOptions::default();

    for r in refs {
        let data = &r.bytes;
        match r.kind.as_str() {
            "paragraph" => {
                let text = String::from_utf8_lossy(data);
                let text_escaped = text
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                paragraph_xml_parts.push(format!(
                    "    <w:p><w:r><w:t>{}</w:t></w:r></w:p>",
                    text_escaped
                ));
            }
            "image" => {
                let name = r
                    .name
                    .clone()
                    .unwrap_or_else(|| "word/media/image-default.bin".to_string());
                zw.start_file(&name, opts).map_err(|e| e.to_string())?;
                zw.write_all(data).map_err(|e| e.to_string())?;
            }
            "font" => {
                let name = r
                    .name
                    .clone()
                    .unwrap_or_else(|| "word/fonts/font-default.bin".to_string());
                zw.start_file(&name, opts).map_err(|e| e.to_string())?;
                zw.write_all(data).map_err(|e| e.to_string())?;
            }
            "xml" => {
                if let Some(name) = &r.name {
                    if !written_files.contains(name) {
                        zw.start_file(name, opts).map_err(|e| e.to_string())?;
                        zw.write_all(data).map_err(|e| e.to_string())?;
                        written_files.insert(name.clone());
                    }
                }
            }
            _ => {}
        }
    }

    // Only regenerate word/document.xml from extracted paragraphs as a FALLBACK
    // for vaults ingested by old parsers that didn't preserve the original.
    // New ingests preserve the bytes verbatim above, so the table structure,
    // sections, run formatting, etc. all survive the round-trip.
    if !written_files.contains("word/document.xml") {
        let document_xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
{}
  </w:body>
</w:document>"#,
            paragraph_xml_parts.join("\n")
        );
        zw.start_file("word/document.xml", opts)
            .map_err(|e| e.to_string())?;
        zw.write_all(document_xml.as_bytes())
            .map_err(|e| e.to_string())?;
    }

    // Write required entries only if not already restored from stored structural XML.
    if !written_files.contains("_rels/.rels") {
        write_zip_entry(&mut zw, "_rels/.rels", default_rels(), opts)?;
    }
    if !written_files.contains("[Content_Types].xml") {
        write_zip_entry(&mut zw, "[Content_Types].xml", default_content_types(), opts)?;
    }
    if !written_files.contains("word/_rels/document.xml.rels") {
        write_zip_entry(
            &mut zw,
            "word/_rels/document.xml.rels",
            default_doc_rels(),
            opts,
        )?;
    }

    let buf = zw.finish().map_err(|e| e.to_string())?;
    Ok(buf.into_inner())
}

fn write_zip_entry(
    zw: &mut zip::ZipWriter<std::io::Cursor<Vec<u8>>>,
    name: &str,
    content: &str,
    opts: zip::write::FileOptions,
) -> Result<(), String> {
    zw.start_file(name, opts).map_err(|e| e.to_string())?;
    zw.write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn default_rels() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#
}

fn default_doc_rels() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>"#
}

fn default_content_types() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#
}
