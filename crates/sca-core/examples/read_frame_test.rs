// Quick probe: try read_frame_by_id on a known-tombstoned frame to see
// where it fails. Run with:
//   cargo run -p sca-core --example read_frame_test -- <path> <frame_id>

use sca_core::said_file::SaidFile;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let frame_id: u64 = args[2].parse().expect("frame id");

    let bytes = std::fs::read(path).expect("read file");
    let mut brain = SaidFile::from_bytes(bytes).expect("parse");

    println!("file: {}, mode: {:?}", path, brain.mode());

    // Walk all frames, look for our target
    let metas = brain.frames.get_all_frames();
    let mut found_meta = None;
    for m in &metas {
        if m.id == frame_id {
            println!("FOUND meta: id={} doc_id={} status={:?} encoding={:?} offset={} compressed_len={} uncompressed_len={}",
                m.id, m.doc_id, m.status, m.encoding, m.offset, m.compressed_len, m.uncompressed_len);
            found_meta = Some((*m).clone());
        }
    }
    if found_meta.is_none() {
        println!("frame {} not in metas — that explains 'not found'", frame_id);
        return;
    }

    // Probe block state
    let blocks = brain.frames.block_count();
    let map_size = brain.frames.block_map_size();
    let has_dict = brain.frames.has_zstd_dict();
    println!("blocks: {}, block_map size: {}, has_zstd_dict: {}", blocks, map_size, has_dict);
    println!("frame {} in block_map: {}", frame_id, brain.frames.block_map_has(frame_id));

    // Dump every block's offset/size
    for (i, b) in brain.frames.blocks_dump().iter().enumerate() {
        println!("  block {}: offset={} comp_len={} uncomp_len={} frame_count={}", i, b.0, b.1, b.2, b.3);
    }

    println!("attempting read_frame_by_id...");
    match brain.read_frame_by_id(frame_id) {
        Some(text) => println!("OK: {} bytes:\n{}", text.len(), text.chars().take(200).collect::<String>()),
        None => println!("read_frame_by_id returned None"),
    }
}
