# File format blueprint — `.said` v7_1

Every `.said` file is a sequence of byte sections, each announced by a 4-byte magic at a deterministic offset (for core sections) or by magic scanning (for optional sections added after v7). The format is designed for mmap-friendly random access: opening a file is a handful of u64 reads, touching a frame is one decompression, and writing is an atomic rewrite.

## Contents

- [2.1 v7_1 header (byte-by-byte)](2.1-v7_1-header.md)
- [2.2 All sections (SCRM / BRAN / MODE / AUDT / TRGM / SYMS / DICT / BLKT / CTXT / FTOC / REFS)](2.2-sections.md)
- [2.3 Frame layout (TOC entry + compressed payload + BLAKE3)](2.3-frame-layout.md)
- [2.4 Block-compressed frames (H.265 GOP-inspired)](2.4-block-compression.md)
- [2.5 Version history + migration rules](2.5-version-history.md)

## High-level layout

```
╔══════════════════════════════════════════════════════════════╗
║  Header (72 bytes, v7_1)                                     ║  ← always at offset 0
║    magic "SAID" | version=7 | flags | frame_count            ║
║    scrm_offset | toc_offset | dict_offset | blkt_offset      ║
║    trgm_offset | syms_offset | refs_offset | reserved        ║
╠══════════════════════════════════════════════════════════════╣
║  Frame blocks (zstd, block-256, BLAKE3'd)                    ║  ← the bulk
║    BLOCK header + compressed frames + ...                    ║
╠══════════════════════════════════════════════════════════════╣
║  DICT   (zstd trained dictionary)                            ║  ← optional, offset in header
║  SCRM   (SCA 1-bit fingerprints)                             ║  ← required for search
║  BRAN   (brain state: S_slow, recall weights, query log)     ║  ← brain memory
║  MODE   (8 bytes: deployment mode)                           ║  ← optional, magic scan
║  AUDT   (append-only audit log)                              ║  ← optional, magic scan
║  TRGM   (trigram inverted index)                             ║  ← optional, offset in header
║  SYMS   (symbol table from AST chunking)                     ║  ← optional, offset in header
║  BLKT   (block table for decompression)                      ║  ← optional, offset in header
║  FTOC   (frame table of contents)                            ║  ← required, offset in header
║  REFS   (cross-file reference edges, reserved for future)    ║  ← not yet populated
╚══════════════════════════════════════════════════════════════╝
```

## Design principles

- **Header-first.** The header holds u64 offsets for every section that needs fast access (frames, SCRM, TOC, DICT, BLKT, TRGM, SYMS, REFS). Open time = constant regardless of file size.
- **Magic-scanned late additions.** Sections added after the header was frozen (`MODE`, `AUDT`) are found by scanning for their magic. Cheap on a mmap'd file; guarantees back-compat for any reader.
- **mmap-native.** The file is designed so every read path is a slice into mmap bytes. No deserialization into heap vectors for hot paths.
- **Append-friendly.** Writes rewrite the whole file (atomic rename) but structure-wise nothing requires this — blocks could be appended in a future v7_2 if the lifecycle demanded it.
- **Version-bump via flag bit, not magic.** The `SAID` magic stays at 7; new sections signal via flag bits on the u16 flags field. Readers that don't know a flag silently skip the new sections.

## Where to start

- Reading an existing `.said`? Start with [2.1 header](2.1-v7_1-header.md).
- Implementing a new section? Start with [2.2 sections](2.2-sections.md) and follow the pattern of the closest analogue (MODE is 8 bytes and simplest; TRGM is compressed and most complex).
- Migrating an older file? Start with [2.5 version history](2.5-version-history.md).

## Constants (from source)

From [`crates/sca-core/src/said_file.rs`](../../../crates/sca-core/src/said_file.rs):

```rust
const SAID_MAGIC: &[u8; 4] = b"SAID";
const SAID_VERSION: u16 = 7;
const HEADER_SIZE_V7: usize = 48;     // legacy, still loadable
const HEADER_SIZE_V7_1: usize = 72;   // current
const FLAG_EXTENDED_HEADER: u16 = 0x0001;
```
