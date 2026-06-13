//! Smoke probe: dump what our xlsx_styles resolver sees for a given file.
//!
//! Run with: `cargo run --features forge-xlsx -p said-forge --example xlsx_probe -- <path>`

#![cfg(feature = "forge-xlsx")]

fn main() {
    let path = std::env::args().nth(1).expect("pass a path to .xlsx/.xlsm");
    let bytes = std::fs::read(&path).expect("read file");
    let styles = said_forge::source::xlsx_styles::load_styles(&bytes)
        .expect("styles")
        .expect("has styles");
    println!("=== WorkbookStyles ===");
    println!("  fills: {}", styles.fills.len());
    for (i, f) in styles.fills.iter().enumerate() {
        println!(
            "    fill[{}]: hex={:?} label={:?}",
            i, f.hex, f.label
        );
    }
    println!("  cell_xfs: {}", styles.cell_xfs.len());
    // Print first 20 xf fill-id assignments
    for (i, fid) in styles.cell_xfs.iter().take(20).enumerate() {
        println!("    xf[{}] fill_id={}", i, fid);
    }

    // Resolve a common sheet
    for candidate in ["Detailed Requirements", "Sheet1", "Requirements"] {
        if let Some(p) = said_forge::source::xlsx_styles::resolve_sheet_path(&bytes, candidate) {
            println!("\n=== sheet '{}' → {}", candidate, p);
            match said_forge::source::xlsx_styles::extract_row_colors(&bytes, &p, &styles) {
                Ok(rc) => {
                    println!("  row_colors entries: {}", rc.len());
                    for (row, color) in rc.iter().take(20) {
                        println!("    row {} → hex={:?} label={:?}", row, color.hex, color.label);
                    }
                }
                Err(e) => println!("  error: {}", e),
            }
        }
    }
}
