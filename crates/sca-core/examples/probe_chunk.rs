//! Diagnostic: dump AST chunks for a single file.
//! Usage: cargo run --release --example probe_chunk --features code,static-embed -- <file>

use std::env;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args().nth(1).expect("usage: probe_chunk <file>");
    let bytes = fs::read(&path)?;
    let payload = if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        &bytes[3..]
    } else {
        &bytes[..]
    };
    let content = String::from_utf8(payload.to_vec())?;
    let ext = std::path::Path::new(&path).extension()
        .and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    println!("file: {} (ext: {}, {} bytes content)", path, ext, content.len());

    #[cfg(feature = "code")]
    {
        let chunks = sca_core::code_search::ast_chunk(&content, &ext);
        println!("ast_chunk returned {} chunks", chunks.len());
        for (i, c) in chunks.iter().enumerate() {
            println!("  [{}] kind={:<25} name={:<30} lines {}-{} ({} bytes)",
                i, c.kind, c.name, c.start_line, c.end_line, c.content.len());
        }
    }
    #[cfg(not(feature = "code"))]
    println!("(built without `code` feature)");
    Ok(())
}
