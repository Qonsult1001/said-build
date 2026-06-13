# said — minimal build repo

Compiles the `said` CLI and `said-mcp` server with code + document support.
This is the minimal set extracted from SAID-ECHO: just the crates needed to
build the two binaries, plus the static encoder model (baked in via
`include_bytes!`). No app/wasm/docs/research baggage.

## Build

```bash
cargo build --release -p said-cli --features "code,docs"   # -> target/release/said
cargo build --release -p said-mcp --features "code"        # -> target/release/said-mcp
```

Native deps for `docs` (PDF): cmake, clang, openssl dev headers (Linux/mac).

## Cloud builds

Push a `v*` tag (or run the **build-binaries** workflow manually) to get
Linux / Windows / macOS binaries as release artifacts. No secrets needed —
the encoder is committed here.
