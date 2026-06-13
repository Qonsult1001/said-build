#![cfg(feature = "whisper")]
//! Video Brain Plugin — transcribe audio/video and store as .said frames.
//!
//! Uses sherpa-rs (bindings to sherpa-onnx):
//!   - Supports Whisper, Moonshine, SenseVoice — all engines
//!   - Prebuilt binaries — no LLVM, no CUDA, no build headaches
//!   - Optional DirectML GPU on any Windows GPU (no CUDA)
//!   - symphonia decodes MP4/MP3 audio; sherpa-rs does the inference
//!
//! Model loaded ONCE, cached for all subsequent transcriptions.
//! Already-indexed files skipped automatically.

use std::path::Path;
use std::sync::Mutex;

use crate::frames::{MemoryKind, MemoryScope, MemorySubject, MemoryType, PutOptions};

// ────────────────────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────────────────────

pub struct MediaSegment {
    pub start_secs: f32,
    pub end_secs: f32,
    pub text: String,
}

pub struct TranscriptionResult {
    pub segments: Vec<MediaSegment>,
    pub duration_secs: f32,
}

pub struct IngestReport {
    pub source_path: String,
    pub duration_secs: f32,
    pub segments_transcribed: usize,
    pub frames_stored: usize,
    pub skipped: bool,
}

// ────────────────────────────────────────────────────────────────────────────
// Audio decode (symphonia — handles MP4/MP3/WAV/FLAC)
// ────────────────────────────────────────────────────────────────────────────

const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Decode audio from video/audio file to 16kHz mono f32 PCM.
fn decode_audio(path: &Path) -> Result<(Vec<f32>, f32), String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path)
        .map_err(|e| format!("Cannot open {}: {}", path.display(), e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("Probe failed: {}", e))?;

    let mut format = probed.format;
    let track = format.tracks().iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| format!("No audio track in {}", path.display()))?;

    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Codec init: {}", e))?;

    let mut samples: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(_) => break,
        };
        if packet.track_id() != track_id { continue; }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let spec = *decoded.spec();
        let num_frames = decoded.frames();
        if num_frames == 0 { continue; }

        let mut sbuf = SampleBuffer::<f32>::new(num_frames as u64, spec);
        sbuf.copy_interleaved_ref(decoded);
        let interleaved = sbuf.samples();

        if channels > 1 {
            for chunk in interleaved.chunks(channels) {
                samples.push(chunk.iter().sum::<f32>() / channels as f32);
            }
        } else {
            samples.extend_from_slice(interleaved);
        }
    }

    let duration_secs = samples.len() as f32 / sample_rate as f32;

    // Resample to 16kHz if needed
    if sample_rate != WHISPER_SAMPLE_RATE {
        samples = resample_linear(&samples, sample_rate, WHISPER_SAMPLE_RATE);
    }

    eprintln!("[whisper] decoded {} — {:.1}s, {} samples @ 16kHz",
        path.display(), duration_secs, samples.len());
    Ok((samples, duration_secs))
}

fn resample_linear(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate { return samples.to_vec(); }
    let ratio = to_rate as f64 / from_rate as f64;
    let out_len = (samples.len() as f64 * ratio).ceil() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 / ratio;
        let idx = pos.floor() as usize;
        let frac = (pos - idx as f64) as f32;
        if idx + 1 < samples.len() {
            out.push(samples[idx] * (1.0 - frac) + samples[idx + 1] * frac);
        } else if idx < samples.len() {
            out.push(samples[idx]);
        }
    }
    out
}

// ────────────────────────────────────────────────────────────────────────────
// Cached recognizer (load once, transcribe many)
// ────────────────────────────────────────────────────────────────────────────

static RECOGNIZER_CACHE: Mutex<Option<sherpa_rs::whisper::WhisperRecognizer>> = Mutex::new(None);

/// Model directory — downloaded once to ./sherpa-models/
const MODEL_DIR: &str = "sherpa-onnx-whisper-tiny";

/// Download Whisper tiny model if not present.
fn ensure_model() -> Result<String, String> {
    let model_dir = MODEL_DIR;
    let encoder = format!("{}/tiny-encoder.onnx", model_dir);

    if std::path::Path::new(&encoder).exists() {
        return Ok(model_dir.to_string());
    }

    eprintln!("[whisper] Downloading whisper-tiny model...");
    let t0 = std::time::Instant::now();

    // Download from sherpa-onnx releases
    let url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-whisper-tiny.tar.bz2";
    let archive_name = "sherpa-onnx-whisper-tiny.tar.bz2";

    // Use hf-hub or direct download
    // For simplicity, check if model files exist in common locations
    let alt_paths = [
        format!("./{}", model_dir),
        format!("../../{}", model_dir),
        format!("../{}", model_dir),
    ];
    for alt in &alt_paths {
        if std::path::Path::new(&format!("{}/tiny-encoder.onnx", alt)).exists() {
            return Ok(alt.clone());
        }
    }

    Err(format!(
        "Whisper model not found. Download it:\n\
         wget {}\n\
         tar xvf {}\n\
         This creates ./{}/",
        url, archive_name, model_dir
    ))
}

fn get_or_create_recognizer() -> Result<std::sync::MutexGuard<'static, Option<sherpa_rs::whisper::WhisperRecognizer>>, String> {
    let mut guard = RECOGNIZER_CACHE.lock().map_err(|e| format!("Lock: {}", e))?;

    if guard.is_some() {
        eprintln!("[whisper] Recognizer cached (reusing)");
        return Ok(guard);
    }

    let model_dir = ensure_model()?;
    eprintln!("[whisper] Loading model from {}/", model_dir);
    let t0 = std::time::Instant::now();

    let config = sherpa_rs::whisper::WhisperConfig {
        encoder: format!("{}/tiny-encoder.onnx", model_dir),
        decoder: format!("{}/tiny-decoder.onnx", model_dir),
        tokens: format!("{}/tiny-tokens.txt", model_dir),
        language: "en".into(),
        provider: None, // auto-detect (DirectML if available, else CPU)
        num_threads: Some(std::thread::available_parallelism()
            .map(|n| n.get() as i32).unwrap_or(4)),
        bpe_vocab: None,
        ..Default::default()
    };

    let recognizer = sherpa_rs::whisper::WhisperRecognizer::new(config)
        .map_err(|e| format!("Recognizer init: {}", e))?;

    let ms = t0.elapsed().as_millis();
    eprintln!("[whisper] Model loaded in {}ms", ms);

    *guard = Some(recognizer);
    Ok(guard)
}

// ────────────────────────────────────────────────────────────────────────────
// Transcription
// ────────────────────────────────────────────────────────────────────────────

/// Transcribe a video/audio file to timestamped segments.
pub fn transcribe(path: &str) -> Result<TranscriptionResult, String> {
    let path_ref = Path::new(path);
    if !path_ref.exists() {
        return Err(format!("File not found: {}", path));
    }

    let (samples, duration_secs) = decode_audio(path_ref)?;

    let mut guard = get_or_create_recognizer()?;
    let recognizer = guard.as_mut().ok_or("Recognizer not loaded")?;

    // sherpa-rs offline Whisper has a HARD 30s buffer limit.
    // We slice to 25s with 2s overlap to avoid truncation at boundaries.
    // The overlap prevents cutting words mid-sentence.
    const CHUNK_SECS: f32 = 25.0;
    const OVERLAP_SECS: f32 = 2.0;
    const CHUNK_SAMPLES: usize = (CHUNK_SECS * WHISPER_SAMPLE_RATE as f32) as usize;
    const STEP_SAMPLES: usize = ((CHUNK_SECS - OVERLAP_SECS) * WHISPER_SAMPLE_RATE as f32) as usize;
    let num_chunks = if samples.len() <= CHUNK_SAMPLES {
        1
    } else {
        (samples.len().saturating_sub(CHUNK_SAMPLES) + STEP_SAMPLES - 1) / STEP_SAMPLES + 1
    };
    let mut segments = Vec::new();

    let t0 = std::time::Instant::now();
    for chunk_idx in 0..num_chunks {
        let start = chunk_idx * STEP_SAMPLES;
        let end = (start + CHUNK_SAMPLES).min(samples.len());
        if start >= samples.len() { break; }
        let chunk = &samples[start..end];

        let chunk_duration = chunk.len() as f32 / WHISPER_SAMPLE_RATE as f32;
        let start_secs = start as f32 / WHISPER_SAMPLE_RATE as f32;
        let end_secs = end as f32 / WHISPER_SAMPLE_RATE as f32;

        // Verify chunk is under 30s (safety check)
        if chunk_duration > 30.0 {
            eprintln!("[whisper] WARNING: chunk {:.1}s exceeds 30s limit, truncating", chunk_duration);
            let truncated = &chunk[..30 * WHISPER_SAMPLE_RATE as usize];
            let result = recognizer.transcribe(WHISPER_SAMPLE_RATE, truncated);
            let text = result.text.trim().to_string();
            if !text.is_empty() {
                segments.push(MediaSegment { start_secs, end_secs: start_secs + 30.0, text });
            }
            continue;
        }

        let result = recognizer.transcribe(WHISPER_SAMPLE_RATE, chunk);
        let text = result.text.trim().to_string();

        if !text.is_empty() {
            eprintln!("[whisper] chunk {}/{} [{:.0}s-{:.0}s] ({:.1}s): {}",
                chunk_idx + 1, num_chunks, start_secs, end_secs, chunk_duration,
                if text.len() > 80 { &text[..80] } else { &text });
            segments.push(MediaSegment {
                start_secs,
                end_secs,
                text,
            });
        }
    }

    let infer_ms = t0.elapsed().as_millis();
    let rtf = infer_ms as f32 / (duration_secs * 1000.0);
    eprintln!("[whisper] {:.1}s audio in {}ms ({:.2}x realtime), {} segments",
        duration_secs, infer_ms, rtf, segments.len());

    Ok(TranscriptionResult { segments, duration_secs })
}

// ────────────────────────────────────────────────────────────────────────────
// Ingest into .said
// ────────────────────────────────────────────────────────────────────────────

pub fn ingest_video(
    brain: &mut crate::said_file::SaidFile,
    video_path: &str,
) -> Result<IngestReport, String> {
    let abs_path = std::fs::canonicalize(video_path)
        .unwrap_or_else(|_| std::path::PathBuf::from(video_path));
    let abs_str = abs_path.to_string_lossy().to_string();
    let source_tag = format!("source:{}", abs_str);

    // Skip if already indexed
    let existing: Vec<String> = brain.frames.active_doc_ids()
        .iter()
        .filter(|id| {
            brain.frames.get_meta(id)
                .map(|m| m.tags.iter().any(|t| t == &source_tag))
                .unwrap_or(false)
        })
        .map(|s| s.to_string())
        .collect();

    if !existing.is_empty() {
        eprintln!("[whisper] SKIP: {} already indexed ({} frames)", video_path, existing.len());
        return Ok(IngestReport {
            source_path: video_path.to_string(),
            duration_secs: 0.0,
            segments_transcribed: 0,
            frames_stored: 0,
            skipped: true,
        });
    }

    let result = transcribe(video_path)?;

    let filename = Path::new(video_path).file_name().unwrap_or_default().to_string_lossy();
    let stem = Path::new(video_path).file_stem().unwrap_or_default().to_string_lossy();
    let total_dur = format!("{:.1}", result.duration_secs);
    let mut frames_stored = 0;

    for (i, seg) in result.segments.iter().enumerate() {
        let doc_id = format!("{}_seg_{:04}", stem, i);
        let s_start = seg.start_secs as u32;
        let s_end = seg.end_secs as u32;
        let title = format!("{} [{:02}:{:02}-{:02}:{:02}]",
            filename, s_start / 60, s_start % 60, s_end / 60, s_end % 60);

        let tags = vec![
            source_tag.clone(),
            format!("ts_start:{:.1}", seg.start_secs),
            format!("ts_end:{:.1}", seg.end_secs),
            "media:video".to_string(),
            format!("duration:{}", total_dur),
            format!("segment:{}", i),
        ];

        let opts = PutOptions::new(&doc_id, &seg.text)
            .with_title(&title)
            .with_type(MemoryType::Episodic)
            .with_kind(MemoryKind::Event)
            .with_subject(MemorySubject::World)
            .with_scope(MemoryScope::Personal)
            .with_tags(tags);

        brain.put_with(&opts);
        frames_stored += 1;
    }

    eprintln!("[whisper] ingested {} — {} segments, {} frames", video_path, result.segments.len(), frames_stored);

    Ok(IngestReport {
        source_path: video_path.to_string(),
        duration_secs: result.duration_secs,
        segments_transcribed: result.segments.len(),
        frames_stored,
        skipped: false,
    })
}
