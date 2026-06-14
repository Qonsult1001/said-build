# Whisper plugin — audio / video transcription

**Feature flag:** `whisper` in [`crates/sca-core/Cargo.toml`](../../../crates/sca-core/Cargo.toml). Optional `directml` for Windows GPU acceleration.

Entry point: [`crates/sca-core/src/whisper_ingest.rs`](../../../crates/sca-core/src/whisper_ingest.rs).

## What it does

Ingests `.mp3`, `.mp4`, `.wav`, `.m4a`, `.flac` files. Transcribes audio (or audio track from video) with timestamped segments; each segment lands as one Episodic frame tagged with its offset.

## Dependencies

```toml
[dependencies.sherpa-rs]   version = "0.6"  optional = true  features = ["download-binaries"]
[dependencies.symphonia]   version = "0.5"  optional = true  default-features = false
                           features = ["aac", "mp3", "isomp4", "wav", "pcm"]
```

Plus optional `directml` — Windows GPU acceleration via DirectML on any GPU (no CUDA needed).

`sherpa-rs` is a Rust binding for [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx), a CPU/GPU inference runtime for Whisper, Moonshine, SenseVoice, and other ASR models. `download-binaries` feature pulls prebuilt sherpa-onnx binaries so users don't need a C++ toolchain.

`symphonia` is pure Rust audio decode. Needed because sherpa-onnx expects WAV / PCM; symphonia converts MP3 / MP4 / AAC / M4A → PCM in-process.

## Pipeline

```
ingest_video(brain, video_path)
  │
  ▼
symphonia.open(video_path)
  ├─ extract audio track (first audio stream)
  ├─ decode MP3/AAC/... → PCM float samples at native sample rate
  └─ resample to 16 kHz mono (Whisper's expected input)
  │
  ▼
sherpa_onnx.transcribe(pcm) → Vec<Segment { start, end, text }>
  // Each segment is typically 5-15 seconds of speech
  │
  ▼
for each segment:
  brain.remember_with_pillar(
    Some(&format!("{}:seg_{}", doc_id, i)),
    &segment.text,
    Some(&format!("{} [{}s-{}s]", title, segment.start, segment.end)),
    Pillar::Episodic,
    vec![
      format!("source:{}", video_path),
      format!("format:video"),
      format!("segment_start:{}", segment.start),
      format!("segment_end:{}", segment.end),
    ],
  )
```

## Supported audio engines (via sherpa-rs)

sherpa-onnx supports multiple ASR families:

- **Whisper** (OpenAI) — highest accuracy, most expensive. Options: tiny/base/small/medium/large
- **Moonshine** (2024) — edge-optimized, smaller & faster than Whisper-tiny
- **SenseVoice** — multilingual, fast, high accuracy on Chinese
- **Paraformer** — streaming-capable Chinese / English

The `whisper_ingest.rs` entry auto-picks based on available model files. Model download is handled by sherpa-rs's `download-binaries` feature.

## DirectML GPU acceleration

With `directml` feature enabled on Windows:

```
cargo build --release -p said-cli --features "static-embed whisper directml"
```

Transcription on a mid-range GPU (GTX 1660) runs **4-8× faster** than CPU. Works with any DirectX 12-compatible GPU — no CUDA, no specific vendor lock-in.

## Outputs

- One frame per transcript segment (typical 1-minute video → 6-12 frames)
- Tags: `source:<path>`, `format:video|audio`, `segment_start:<s>`, `segment_end:<s>`, `pillar:episodic`
- Frame body = plain transcript text; offset info lives in tags + title

Search returns segment frames; `said ask "X"` surfaces the relevant snippet with its time offset.

## Performance

Observed with Whisper-small model:

| Hardware | 1-minute audio |
|---|---|
| CPU only (AMD Ryzen 7) | ~8 sec |
| DirectML GPU (GTX 1660) | ~1 sec |
| Moonshine-base on CPU | ~3 sec |

So transcribing a 1-hour podcast takes 60 sec on GPU, 4 min on CPU with Moonshine.

## How to test

```
cargo run --release -p said-cli --features "static-embed whisper" -- \
    --path test.said ingest sample.mp4

# Expected: "Ingested: sample.mp4, 45 segments, 45 frames"
said --path test.said ask "what was discussed about X"
# → returns the relevant segment with time offset
```

From the Step 0 video-brain proof-of-architecture tests (April 2025): a 1-hour meeting recording indexed in 12 seconds on GPU; `said ask` finds specific discussion moments at sub-10ms retrieval latency.

## How to extend

### Add a new ASR model family
sherpa-onnx already supports most of them. Switching is a model-file path change. `whisper_ingest.rs` could accept an `AsrEngine` enum for explicit selection.

### Speaker diarization
sherpa-onnx offers speaker embedding models. Adding diarization = second pass that clusters segment embeddings, tag each segment with `speaker:<id>`. Not shipped.

### Realtime / streaming transcription
sherpa-onnx supports streaming. `said listen` would open the mic, transcribe live, append segments to a growing Episodic brain. Not shipped — would need a daemon-mode CLI.

## Known limitations

- Whisper / Moonshine models are English-biased; non-English accuracy varies
- DirectML is Windows-only; macOS / Linux GPUs fall back to CoreML / CUDA / CPU
- Subtitle files (.srt, .vtt) aren't auto-ingested; only audio tracks
- Speaker identification requires a second pass that's not shipped
- No streaming / mic mode

## See also

- [whisper_ingest source](../../../crates/sca-core/src/whisper_ingest.rs)
- [sherpa-onnx project](https://github.com/k2-fsa/sherpa-onnx)
- [Episodic pillar](../04-four-pillars/episodic.md) — where transcripts land
