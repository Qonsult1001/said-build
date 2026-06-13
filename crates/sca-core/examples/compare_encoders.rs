//! Encoder A/B — `said-lam-static` (64-dim, production) vs MinishLab's
//! `potion-retrieval-32M` (512-dim). For each test question:
//!   1. Encode the query with both encoders.
//!   2. Encode every active frame in the brain with both.
//!   3. Brute-force cosine over all frames per encoder.
//!   4. Print top-10 frames per encoder side-by-side.
//!   5. Report the rank of any expected-substring doc under each.
//!
//! NO ranking heuristics, NO temporal layer, NO recall floor. Pure
//! query-against-frame cosine. The point is to isolate the encoder's
//! ability to put the right doc near the top.
//!
//! Run:
//!   PATH_SAID=willie.said
//!   cargo run --release --example compare_encoders --features static-embed
//!
//! First run downloads `potion-retrieval-32M` from HuggingFace (~32MB
//! f32, cached to ~/.cache/huggingface).

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use model2vec_rs::model::StaticModel as PotionModel;
use sca_core::latent_cluster::StaticEncoder as SaidEncoder;
use sca_core::said_file::SaidFile;

const REPORT_PATH: &str = "research/MinishLab/encoder_compare.md";
const TOP_N: usize = 10;

// Same questions as baseline_recall.rs (Section 1).
struct TestCase {
    id: &'static str,
    question: &'static str,
    note: &'static str,
    /// Expected doc-id substrings (any one in top-N counts as found).
    expect: &'static [&'static str],
}

const CASES: &[TestCase] = &[
    TestCase {
        id: "S2.1",
        question: "What is the Tier 1 financial threshold?",
        note: "Above $25,000,000 — Bylaws_v7",
        expect: &["Bylaws_v7"],
    },
    TestCase {
        id: "S2.2",
        question: "Who signed the Vendor SLA for Project Prometheus?",
        note: "Brandt + Hanni — Vendor_SLA_Prometheus",
        expect: &["Vendor_SLA_Prometheus"],
    },
    TestCase {
        id: "S2.3",
        question: "When was Zephyr Holdings Ltd incorporated?",
        note: "12 February 2024 — Companies_House_Search_Zephyr",
        expect: &["Companies_House_Search_Zephyr"],
    },
    TestCase {
        id: "S2.4",
        question: "What date was the Emergency Board Minutes meeting?",
        note: "May 14, 2024 — Emergency_Board_Minutes_May_2024",
        expect: &["Emergency_Board_Minutes"],
    },
    TestCase {
        id: "1.1",
        question: "What happened with R.C. Hanni in May 2024?",
        note: "Slack_Export_IT_May2024 / Emergency_Board_Minutes_May",
        expect: &["Slack_Export_IT_May2024", "Emergency_Board_Minutes_May"],
    },
    TestCase {
        id: "1.2",
        question: "What happened with R.C. Hanni in June 2024?",
        note: "Vendor_SLA_Prometheus_June2024",
        expect: &["Vendor_SLA_Prometheus_June"],
    },
    TestCase {
        id: "1.3",
        question: "What is the most recent thing about Project Prometheus?",
        note: "Vendor_SLA_Prometheus_June2024 (chronologically latest)",
        expect: &["Vendor_SLA_Prometheus_June"],
    },
    TestCase {
        id: "1.4",
        question: "What was discussed about Hanni earlier this year?",
        note: "Any Hanni / Zephyr / Vendor_SLA / Slack_Export",
        expect: &["Hanni", "Zephyr", "Vendor_SLA", "Slack_Export"],
    },
    TestCase {
        id: "1.5",
        question: "Tell me about events from before May 2024",
        note: "Q1 board minutes / Bylaws / Companies_House",
        expect: &["Board_Minutes_Q1", "Bylaws_v7", "Companies_House"],
    },
    TestCase {
        id: "1.6",
        question: "What happened in 1998?",
        note: "Dogman / JHW065 (1998-02-01 RC Hanni line)",
        expect: &["Dogman", "JHW065", "1998"],
    },
    TestCase {
        id: "1.7",
        question: "What happened on 14 May 2024 specifically?",
        note: "Slack_Export May 14",
        expect: &["Slack_Export_IT_May2024", "Emergency_Board_Minutes_May"],
    },
    TestCase {
        id: "1.8",
        question: "Show me events from yesterday",
        note: "No expected match — engine should not confabulate",
        expect: &[],
    },
];

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb).max(1e-9)
}

/// Tokenise a doc_id into a flat space-separated string. Splits on
/// `_`, `/`, `.`, `::`, `-`. Lowercases. Filters tokens of length < 2.
/// Example:
///   "Vendor_SLA_Prometheus_June2024.txt::memory_0044"
///   -> "vendor sla prometheus june2024 txt memory 0044"
fn tokenize_doc_id(doc_id: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    for c in doc_id.chars() {
        if c.is_ascii_alphanumeric() {
            buf.push(c.to_ascii_lowercase());
        } else if !buf.is_empty() {
            if buf.len() >= 2 {
                out.push(std::mem::take(&mut buf));
            } else {
                buf.clear();
            }
        }
    }
    if buf.len() >= 2 {
        out.push(buf);
    }
    out.join(" ")
}

/// Build the prefixed input: "[doc:<tokens>] <content>".
fn prefix_with_doc_id(doc_id: &str, content: &str) -> String {
    let tokens = tokenize_doc_id(doc_id);
    if tokens.is_empty() {
        content.to_string()
    } else {
        format!("[doc:{}] {}", tokens, content)
    }
}

/// Top-N indexed by similarity to query, descending. Stable on ties.
fn topn(query_emb: &[f32], frame_embs: &[Vec<f32>], n: usize) -> Vec<(usize, f32)> {
    let mut scored: Vec<(usize, f32)> = frame_embs
        .iter()
        .enumerate()
        .map(|(i, e)| (i, cosine(query_emb, e)))
        .collect();
    // Sort by score desc, then by index asc for determinism.
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.truncate(n);
    scored
}

/// Find the rank of the FIRST expected-substring match in a top-N list.
/// Returns Some(rank, doc_id) if found within the list, None otherwise.
fn find_expected_rank(
    top: &[(usize, f32)],
    doc_ids: &[String],
    expect: &[&str],
) -> Option<(usize, String)> {
    if expect.is_empty() {
        return None;
    }
    for (rank, (idx, _)) in top.iter().enumerate() {
        let id = &doc_ids[*idx];
        if expect.iter().any(|s| id.contains(s)) {
            return Some((rank + 1, id.clone()));
        }
    }
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path_said = env::var("PATH_SAID").unwrap_or_else(|_| "willie.said".into());
    println!("=== compare_encoders ===");
    println!("brain: {}", path_said);
    println!();

    // ── Load all three encoders ────────────────────────────────────────
    // (1) said-lam-static via our crate \u2014 production today.
    println!("Loading said-lam-static (production, 64-dim) via model2vec-said...");
    let t0 = Instant::now();
    let said_enc = SaidEncoder::from_pretrained("./SAID-LAM-private/said-lam-static")?;
    println!("  loaded in {} ms", t0.elapsed().as_millis());

    // (2) potion-retrieval-32M via MinishLab's crate \u2014 reference impl.
    println!("Loading potion-retrieval-32M via model2vec-rs (MinishLab)...");
    let t0 = Instant::now();
    let potion = PotionModel::from_pretrained(
        "minishlab/potion-retrieval-32M",
        None,
        None,
        None,
    )?;
    println!("  loaded in {} ms", t0.elapsed().as_millis());

    // (3) potion-retrieval-32M via OUR crate \u2014 the actual question.
    // If `model2vec-said::StaticModel::from_path` can read the same
    // safetensors+tokenizer+config layout, we get all of potion's quality
    // without depending on `model2vec-rs`.
    println!("Loading potion-retrieval-32M via model2vec-said (compat probe)...");
    let t0 = Instant::now();
    let potion_via_ours = SaidEncoder::from_pretrained(
        "./research/MinishLab/models/potion-retrieval-32M",
    )?;
    println!("  loaded in {} ms", t0.elapsed().as_millis());
    println!();

    // ── Open brain + read every active frame ───────────────────────────
    let mut brain = SaidFile::open(PathBuf::from(&path_said))?;
    let doc_ids: Vec<String> = brain
        .frames
        .active_doc_ids()
        .iter()
        .map(|s| s.to_string())
        .collect();
    println!("Reading {} active frames from brain...", doc_ids.len());
    let t0 = Instant::now();
    let mut contents_plain: Vec<String> = Vec::with_capacity(doc_ids.len());
    let mut contents_prefixed: Vec<String> = Vec::with_capacity(doc_ids.len());
    for id in &doc_ids {
        let body = brain.read(id).unwrap_or_default();
        // Truncate to 1500 chars to keep encoding bounded — the first chunk
        // of a frame dominates the semantic vector either way.
        let trimmed: String = body.chars().take(1500).collect();
        contents_prefixed.push(prefix_with_doc_id(id, &trimmed));
        contents_plain.push(trimmed);
    }
    println!("  read in {} ms", t0.elapsed().as_millis());
    // Show one prefixed example so we can eyeball that tokenisation worked.
    if let Some((id, prefixed)) = doc_ids.iter().zip(contents_prefixed.iter()).next() {
        let preview: String = prefixed.chars().take(120).collect();
        println!("  example doc_id: {}", id);
        println!("  example prefixed: {}…", preview);
    }

    // ── Encode every frame, four times: said|potion × plain|prefixed ──
    println!("Encoding said-lam-static (plain)...");
    let t0 = Instant::now();
    let said_plain: Vec<Vec<f32>> = said_enc.encode_batch(&contents_plain);
    let said_dim = said_plain.first().map(|v| v.len()).unwrap_or(0);
    println!("  {} ms", t0.elapsed().as_millis());

    println!("Encoding said-lam-static (prefixed)...");
    let t0 = Instant::now();
    let said_pref: Vec<Vec<f32>> = said_enc.encode_batch(&contents_prefixed);
    println!("  {} ms", t0.elapsed().as_millis());

    println!("Encoding potion-retrieval-32M (plain)...");
    let t0 = Instant::now();
    let potion_plain: Vec<Vec<f32>> = potion.encode_with_args(&contents_plain, Some(512), 256);
    let potion_dim = potion_plain.first().map(|v| v.len()).unwrap_or(0);
    println!("  {} ms", t0.elapsed().as_millis());

    println!("Encoding potion-retrieval-32M (prefixed) via model2vec-rs...");
    let t0 = Instant::now();
    let potion_pref: Vec<Vec<f32>> = potion.encode_with_args(&contents_prefixed, Some(512), 256);
    println!("  {} ms", t0.elapsed().as_millis());

    println!("Encoding potion-retrieval-32M (prefixed) via model2vec-said (compat probe)...");
    let t0 = Instant::now();
    let potion_via_ours_pref: Vec<Vec<f32>> = potion_via_ours.encode_batch(&contents_prefixed);
    let pvo_dim = potion_via_ours_pref.first().map(|v| v.len()).unwrap_or(0);
    println!("  {} ms ({} -dim)", t0.elapsed().as_millis(), pvo_dim);
    println!();

    // Sanity: do the two crates produce numerically equivalent vectors?
    // We don't expect bit-identical (different float ordering, different
    // batch handling) but cosine should be near-1 for the same input.
    if let (Some(a), Some(b)) = (potion_pref.first(), potion_via_ours_pref.first()) {
        if a.len() == b.len() {
            let cos = cosine(a, b);
            println!("Sanity: cos(potion_via_rs_frame_0, potion_via_ours_frame_0) = {:.4}", cos);
        } else {
            println!("Sanity: dim mismatch — rs={} ours={}", a.len(), b.len());
        }
    }
    println!();

    // ── For each question: encode query both ways, run cosine top-N,
    //    print side-by-side, record rank of expected-substring doc ─────
    let mut report = String::new();
    report.push_str("# Encoder A/B — plain vs doc_id-prefixed\n\n");
    report.push_str(&format!(
        "Brain: `{}` · {} active frames · {} questions · pure cosine, NO ranking heuristics, NO LLM\n\n",
        path_said, doc_ids.len(), CASES.len(),
    ));
    report.push_str(&format!(
        "- `said-plain` / `said-pref`: said-lam-static {}-dim, content vs `[doc:<tokens>] <content>`\n",
        said_dim
    ));
    report.push_str(&format!(
        "- `potion-plain` / `potion-pref`: potion-retrieval-32M {}-dim, same split.\n\n",
        potion_dim
    ));
    report.push_str("**Hypothesis**: prefixing the encoded text with tokenised doc_id ");
    report.push_str("rescues queries whose answer lives in the filename (e.g. ");
    report.push_str("`Vendor_SLA_Prometheus_June2024.txt`) when the body alone has no date.\n\n");

    // Counters for the five conditions.
    let mut counts: [(u32, u32, u32); 5] = [(0, 0, 0); 5]; // (found, top3, top5)
    let n_with_expect = CASES.iter().filter(|c| !c.expect.is_empty()).count();

    println!("{:<6} {:<44} {:<9} {:<9} {:<9} {:<9} {:<9}",
        "ID", "Question", "s-plain", "s-pref", "p-plain", "p-pref", "p-ours");
    println!("{}", "-".repeat(105));
    report.push_str("| ID | Question | said-plain | said-pref | potion-plain | potion-pref | potion-via-ours-pref |\n");
    report.push_str("|---|---|---|---|---|---|---|\n");

    for case in CASES {
        // Encode the query separately per encoder (frame side carries the prefix; query doesn't).
        let q_said   = said_enc.encode_one(case.question);
        let q_potion = potion.encode(&[case.question.to_string()])[0].clone();
        // For potion-via-ours we use OUR crate to encode the query too \u2014 same
        // tokeniser + lookup table, just the loader/runtime differs.
        let q_potion_ours = potion_via_ours.encode_one(case.question);

        let top_a = topn(&q_said,        &said_plain,             TOP_N);
        let top_b = topn(&q_said,        &said_pref,              TOP_N);
        let top_c = topn(&q_potion,      &potion_plain,           TOP_N);
        let top_d = topn(&q_potion,      &potion_pref,            TOP_N);
        let top_e = topn(&q_potion_ours, &potion_via_ours_pref,   TOP_N);

        let mut cell_for = |top: &[(usize, f32)], col: usize| -> String {
            if let Some((r, _)) = find_expected_rank(top, &doc_ids, case.expect) {
                counts[col].0 += 1;
                if r <= 3 { counts[col].1 += 1; }
                if r <= 5 { counts[col].2 += 1; }
                format!("rank {}", r)
            } else if case.expect.is_empty() {
                "—".to_string()
            } else {
                format!(">{}", TOP_N)
            }
        };
        let a = cell_for(&top_a, 0);
        let b = cell_for(&top_b, 1);
        let c = cell_for(&top_c, 2);
        let d = cell_for(&top_d, 3);
        let e = cell_for(&top_e, 4);

        let q_short = if case.question.len() > 42 {
            format!("{}…", &case.question[..41])
        } else {
            case.question.to_string()
        };
        println!("{:<6} {:<44} {:<9} {:<9} {:<9} {:<9} {:<9}",
            case.id, q_short, a, b, c, d, e);
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            case.id, case.question.replace('|', r"\|"), a, b, c, d, e,
        ));

        // Per-question top-10 detail in markdown.
        report.push_str(&format!("\n### {} — {}\n\n_{}_\n\n", case.id, case.question, case.note));
        for (label, top) in [
            ("said-plain",         top_a),
            ("said-pref",          top_b),
            ("potion-plain",       top_c),
            ("potion-pref",        top_d),
            ("potion-via-ours-pf", top_e),
        ] {
            report.push_str(&format!("**{}** top-{}:\n\n", label, TOP_N));
            for (rank, (idx, sim)) in top.iter().enumerate() {
                let id = &doc_ids[*idx];
                let star = if case.expect.iter().any(|s| id.contains(s)) { "★ " } else { "  " };
                report.push_str(&format!("{}{}. `{}` · cos={:.4}\n\n", star, rank + 1, id, sim));
            }
        }
    }
    println!();

    // Summary rows.
    let labels = ["said-plain", "said-pref", "potion-plain", "potion-pref", "potion-via-ours-pf"];
    println!("Summary over {} questions with expected docs:", n_with_expect);
    for (i, lbl) in labels.iter().enumerate() {
        println!("  {:<22} found={:>2}/{}  top-3={:>2}/{}  top-5={:>2}/{}",
            lbl, counts[i].0, n_with_expect, counts[i].1, n_with_expect, counts[i].2, n_with_expect);
    }
    report.push_str(&format!("\n## Summary\n\nOver {} questions with expected docs:\n\n", n_with_expect));
    report.push_str("| Condition | found in top-10 | top-3 | top-5 |\n");
    report.push_str("|---|---|---|---|\n");
    for (i, lbl) in labels.iter().enumerate() {
        report.push_str(&format!(
            "| {} | {} / {} | {} / {} | {} / {} |\n",
            lbl, counts[i].0, n_with_expect, counts[i].1, n_with_expect, counts[i].2, n_with_expect,
        ));
    }

    fs::create_dir_all("research/MinishLab")?;
    fs::write(REPORT_PATH, &report)?;
    println!();
    println!("wrote {}", REPORT_PATH);

    Ok(())
}
