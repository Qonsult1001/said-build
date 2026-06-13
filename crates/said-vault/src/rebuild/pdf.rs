pub fn rebuild(refs: &[crate::rebuild::RebuildRef]) -> Result<Vec<u8>, String> {
    let mut text_blocks: Vec<(String, f64, f64, f64)> = Vec::new(); // (text, x, y, size)

    for r in refs {
        let data = &r.bytes;
        if r.kind.as_str() == "paragraph" {
            let text = String::from_utf8_lossy(data).to_string();
            // RebuildRef carries no position/size metadata at this layer;
            // the T13 orchestrator may encode them in extra fields in future.
            // For now default to sensible layout values (matching vault-rust
            // defaults when metadata was absent).
            let x: f64 = 72.0;
            let y: f64 = 0.0; // 0 triggers auto_y in build_pdf
            let font_size: f64 = 12.0;
            text_blocks.push((text, x, y, font_size));
        }
    }

    Ok(build_pdf(&text_blocks))
}

fn append(buf: &mut Vec<u8>, text: &str) {
    buf.extend_from_slice(text.as_bytes());
}

fn build_pdf(text_blocks: &[(String, f64, f64, f64)]) -> Vec<u8> {
    let mut buf = Vec::<u8>::new();
    let mut obj_offsets: Vec<usize> = Vec::new();

    append(&mut buf, "%PDF-1.4\n");

    // Build content stream first so length is known before writing the object header.
    let mut content_parts: Vec<String> = Vec::new();
    let mut auto_y = 700.0f64;
    for (text, x, y, font_size) in text_blocks {
        let pos_y = if *y == 0.0 { auto_y } else { *y };
        auto_y -= 20.0;
        content_parts.push(format!(
            "BT\n/F1 {:.1} Tf\n{:.1} {:.1} Td\n({}) Tj\nET",
            font_size,
            x,
            pos_y,
            pdf_escape(text)
        ));
    }
    let content_stream = content_parts.join("\n");

    obj_offsets.push(buf.len());
    append(&mut buf, "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    obj_offsets.push(buf.len());
    append(
        &mut buf,
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
    );

    obj_offsets.push(buf.len());
    append(&mut buf, "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n");

    obj_offsets.push(buf.len());
    append(
        &mut buf,
        &format!(
            "4 0 obj\n<< /Length {} >>\nstream\n",
            content_stream.len() + 1
        ),
    );
    append(&mut buf, &content_stream);
    append(&mut buf, "\nendstream\nendobj\n");

    obj_offsets.push(buf.len());
    append(
        &mut buf,
        "5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
    );

    let xref_pos = buf.len();
    let obj_count = obj_offsets.len() + 1;
    append(&mut buf, &format!("xref\n0 {}\n", obj_count));
    append(&mut buf, &format!("{:010} 65535 f \n", 0));
    for offset in &obj_offsets {
        append(&mut buf, &format!("{:010} 00000 n \n", offset));
    }
    append(
        &mut buf,
        &format!("trailer\n<< /Size {} /Root 1 0 R >>\n", obj_count),
    );
    append(&mut buf, &format!("startxref\n{}\n%%EOF\n", xref_pos));

    buf
}

fn pdf_escape(text: &str) -> String {
    text.replace('\\', r"\\")
        .replace('(', r"\(")
        .replace(')', r"\)")
}
