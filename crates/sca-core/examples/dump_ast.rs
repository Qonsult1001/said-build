//! Dump the tree-sitter AST for a file. Args: file, optional line-range.
//! Usage: cargo run --release --example dump_ast --features code,static-embed -- <file> [start_line] [end_line]

use std::env;
use std::fs;

#[cfg(feature = "code")]
fn dump(node: tree_sitter::Node, source: &[u8], depth: usize, line_lo: usize, line_hi: usize) {
    let start = node.start_position().row + 1;
    let end = node.end_position().row + 1;
    if end < line_lo || start > line_hi { return; }
    let kind = node.kind();
    let preview: String = std::str::from_utf8(&source[node.byte_range()])
        .unwrap_or("")
        .chars().take(60).collect::<String>().replace('\n', " ");
    println!("{}{} [{}-{}] :: {}", "  ".repeat(depth), kind, start, end, preview);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        dump(child, source, depth + 1, line_lo, line_hi);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).ok_or("usage: dump_ast <file> [start] [end]")?;
    let lo: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let hi: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
    let bytes = fs::read(path)?;
    let payload = if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        &bytes[3..]
    } else { &bytes[..] };
    let content = String::from_utf8_lossy(payload).into_owned();
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");

    #[cfg(feature = "code")]
    {
        use tree_sitter::Parser;
        let mut parser = Parser::new();
        // Look up grammar via the central registry — adding a language to
        // sca_core::grammars enables it for this tool with no edits here.
        let spec = match sca_core::grammars::lookup_by_extension(ext) {
            Some(s) => s,
            None => { println!("unsupported ext: {}", ext); return Ok(()); }
        };
        let lang: tree_sitter::Language = (spec.language.0)();
        parser.set_language(&lang)?;
        let tree = parser.parse(&content, None).ok_or("parse failed")?;
        dump(tree.root_node(), content.as_bytes(), 0, lo, hi);
    }
    #[cfg(not(feature = "code"))]
    println!("(built without `code` feature)");
    Ok(())
}
