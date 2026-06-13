//! Code-corpus encoder A/B — `said-lam-static` (production 64-dim) vs
//! THREE potion variants from MinishLab:
//!   - potion-retrieval-32M    (general English retrieval)
//!   - potion-code-16M         (code-specialised, what semble uses)
//!   - potion-multilingual-128M (101 languages — multi-language story)
//!
//! All four loaded via our existing `model2vec-said` crate (proven
//! cos=1.0 compatible with model2vec-rs in compare_encoders.rs).
//!
//! For each test question:
//!   1. Encode the query with all 4 encoders.
//!   2. Encode every active frame in code.said with all 4 (doc_id prefixed).
//!   3. Brute-force cosine over all frames per encoder.
//!   4. Print top-10 frames per encoder side-by-side.
//!   5. Report rank of expected file under each.
//!
//! Pure cosine, NO temporal layer, NO recall floor, NO ranking heuristics.
//!
//! Run:
//!   PATH_SAID=code.said
//!   cargo run --release --example code_compare_encoders --features static-embed
//!
//! First run downloads the 3 potion variants to research/MinishLab/models/
//! (~32MB + ~16MB + ~128MB ≈ ~180MB total disk).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use model2vec_rs::model::StaticModel as PotionModel;
use sca_core::latent_cluster::StaticEncoder as SaidEncoder;
use sca_core::said_file::SaidFile;

const REPORT_PATH: &str = "research/MinishLab/code_encoder_compare.md";
const TOP_N: usize = 10;
const MODELS_ROOT: &str = "research/MinishLab/models";

// ---------------------------------------------------------------------
// Test cases — drafted from inspecting the C# + SQL corpus structure.
// Mix of natural-language and symbol-shaped queries; mix of C# and SQL
// targets. `expect` is doc_id substrings — any one in top-N = pass.
// ---------------------------------------------------------------------
struct TestCase {
    id: &'static str,
    question: &'static str,
    note: &'static str,
    /// Single-language: any one substring in top-N = pass.
    expect: &'static [&'static str],
    /// Cross-language: requires AT LEAST ONE C# match AND at least one
    /// SQL match in top-N. Empty for single-language tests.
    expect_cs:  &'static [&'static str],
    expect_sql: &'static [&'static str],
}

const CASES: &[TestCase] = &[
    // ---- C# natural language -------------------------------------------
    TestCase {
        id: "C1",
        question: "Where is the account balance retrieved from the database?",
        note: "Should hit GetAccountBalanceQuery.cs or related repository",
        expect: &["GetAccountBalanceQuery", "AccountController"],
        expect_cs: &[], expect_sql: &[],
    },
    TestCase {
        id: "C2",
        question: "Find the controller that handles webhook registration",
        note: "WebhookController.cs",
        expect: &["WebhookController", "WebhookService"],
        expect_cs: &[], expect_sql: &[],
    },
    TestCase {
        id: "C3",
        question: "Which class manages cards in the API?",
        note: "CardManagerService.cs / CardController.cs",
        expect: &["CardManagerService", "CardController", "ICardManagerService"],
        expect_cs: &[], expect_sql: &[],
    },
    // ---- C# symbol-shaped (semble's strength claim) --------------------
    TestCase {
        id: "C4",
        question: "ICardManagerService",
        note: "Symbol lookup — exact interface name",
        expect: &["ICardManagerService"],
        expect_cs: &[], expect_sql: &[],
    },
    TestCase {
        id: "C5",
        question: "DefaultDbContext",
        note: "EF Core context class",
        expect: &["DefaultDbContext"],
        expect_cs: &[], expect_sql: &[],
    },
    TestCase {
        id: "C6",
        question: "UpdateAccountBalanceQuery",
        note: "Symbol-shaped query name",
        expect: &["UpdateAccountBalanceQuery"],
        expect_cs: &[], expect_sql: &[],
    },
    // ---- SQL natural language ------------------------------------------
    TestCase {
        id: "S1",
        question: "Find the stored procedure that updates a product",
        note: "p_txn_Api_Update_Product.sql",
        expect: &["p_txn_Api_Update_Product", "Update_Product"],
        expect_cs: &[], expect_sql: &[],
    },
    TestCase {
        id: "S2",
        question: "Where is the idempotency check for API calls?",
        note: "p_txn_Check_Idompotency.sql (note: misspelled in source)",
        expect: &["Check_Idompotency", "Check_Idempotency"],
        expect_cs: &[], expect_sql: &[],
    },
    TestCase {
        id: "S3",
        question: "Which stored procedure creates a new cardholder?",
        note: "p_txn_Create_Cardholder.sql",
        expect: &["p_txn_Create_Cardholder", "Create_Cardholder"],
        expect_cs: &[], expect_sql: &[],
    },
    // ---- SQL symbol-shaped ---------------------------------------------
    TestCase {
        id: "S4",
        question: "p_txn_Get_Cardholder_Cards",
        note: "Exact stored proc name",
        expect: &["p_txn_Get_Cardholder_Cards"],
        expect_cs: &[], expect_sql: &[],
    },
    TestCase {
        id: "S5",
        question: "ala_Api_Live_Audit",
        note: "Audit table by prefixed name",
        expect: &["ala_Api_Live_Audit"],
        expect_cs: &[], expect_sql: &[],
    },
    TestCase {
        id: "S6",
        question: "What does the API live audit table store?",
        note: "Natural-language version of S5 — same target",
        expect: &["ala_Api_Live_Audit"],
        expect_cs: &[], expect_sql: &[],
    },
    // ---- Cross-language questions (X1-X6) ------------------------------
    // Pass criterion: at least one C# match AND at least one SQL match
    // in top-10. `expect` left empty so single-language scoring skips.
    TestCase {
        id: "X1",
        question: "Which API controller updates products and what stored proc does it call?",
        note: "ProductController.cs + p_txn_Api_Update_Product.sql",
        expect: &[],
        expect_cs:  &["ProductController"],
        expect_sql: &["p_txn_Api_Update_Product", "Update_Product"],
    },
    TestCase {
        id: "X2",
        question: "Find the cardholder creation flow — C# service and SQL proc",
        note: "CardController/CardManagerService + p_txn_Create_Cardholder",
        expect: &[],
        expect_cs:  &["CardController", "CardManagerService", "ICardManagerService"],
        expect_sql: &["p_txn_Create_Cardholder", "Create_Cardholder"],
    },
    TestCase {
        id: "X3",
        question: "Where is the audit logged — both API code and database table?",
        note: "Audit/Logging C# + ala_Api_Live_Audit table",
        expect: &[],
        expect_cs:  &["Audit", "Logging"],
        expect_sql: &["ala_Api_Live_Audit"],
    },
    TestCase {
        id: "X4",
        question: "How is the account balance retrieved — query class and stored procedure?",
        note: "GetAccountBalanceQuery.cs + p_txn_API_Get_Account_Balance",
        expect: &[],
        expect_cs:  &["GetAccountBalanceQuery", "AccountController"],
        expect_sql: &["p_txn_API_Get_Account_Balance", "Account_Balance"],
    },
    TestCase {
        id: "X5",
        question: "Webhook registration — controller, service, table",
        note: "WebhookController/WebhookService + nwr_Notification_Webhook_Request",
        expect: &[],
        expect_cs:  &["WebhookController", "WebhookService"],
        expect_sql: &["nwr_Notification_Webhook_Request", "Store_Register_Webhook"],
    },
    TestCase {
        id: "X6",
        question: "Idempotency check end-to-end across C# and SQL",
        note: "Idempotency in C# + Check_Idompotency stored proc",
        expect: &[],
        expect_cs:  &["Idempotency", "Idempotent"],
        expect_sql: &["Check_Idompotency", "Idempotency", "Idompotency"],
    },
];

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb).max(1e-9)
}

fn topn(query_emb: &[f32], frame_embs: &[Vec<f32>], n: usize) -> Vec<(usize, f32)> {
    let mut scored: Vec<(usize, f32)> = frame_embs
        .iter()
        .enumerate()
        .map(|(i, e)| (i, cosine(query_emb, e)))
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.truncate(n);
    scored
}

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

fn prefix_with_doc_id(doc_id: &str, content: &str) -> String {
    let tokens = tokenize_doc_id(doc_id);
    if tokens.is_empty() {
        content.to_string()
    } else {
        format!("[doc:{}] {}", tokens, content)
    }
}

/// Ensure a potion variant is unpacked at MODELS_ROOT/<local_name>.
/// First-time fetch via model2vec-rs, then copy from HF cache to the
/// project-relative path so model2vec-said::from_pretrained can find it.
fn ensure_potion(hf_name: &str, local_name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dest = Path::new(MODELS_ROOT).join(local_name);
    let needed = ["model.safetensors", "tokenizer.json", "config.json"];
    if needed.iter().all(|f| dest.join(f).exists()) {
        return Ok(dest);
    }
    println!("[fetch] {} (first run, downloading to HF cache)...", hf_name);
    // Triggers download to ~/.cache/huggingface/hub/
    let _ = PotionModel::from_pretrained(hf_name, None, None, None)?;

    // Locate the snapshot dir.
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .map_err(|_| "no HOME or USERPROFILE")?;
    let hf_dir = format!("models--{}", hf_name.replace('/', "--"));
    let snapshots_root = Path::new(&home)
        .join(".cache").join("huggingface").join("hub")
        .join(&hf_dir).join("snapshots");
    let snap = fs::read_dir(&snapshots_root)?
        .filter_map(|e| e.ok())
        .next()
        .ok_or_else(|| format!("no snapshot in {}", snapshots_root.display()))?;
    let snap_path = snap.path();

    fs::create_dir_all(&dest)?;
    for fname in &needed {
        let src = snap_path.join(fname);
        let dst = dest.join(fname);
        if !src.exists() {
            return Err(format!("expected {} in HF snapshot, not found", fname).into());
        }
        fs::copy(&src, &dst)?;
    }
    println!("  ready at {}", dest.display());
    Ok(dest)
}

// One row of (label, encoder, frame embeddings, dim).
struct EncoderRun {
    label: &'static str,
    enc: SaidEncoder,
    frame_embs: Vec<Vec<f32>>,
    dim: usize,
    encode_ms: u128,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path_said = env::var("PATH_SAID").unwrap_or_else(|_| "code.said".into());
    println!("=== code_compare_encoders ===");
    println!("brain: {}", path_said);
    println!();

    // ── Load said.lam.static ───────────────────────────────────────────
    println!("Loading said-lam-static (64-dim, production)...");
    let t0 = Instant::now();
    let said_enc = SaidEncoder::from_pretrained("./SAID-LAM-private/said-lam-static")?;
    println!("  loaded in {} ms", t0.elapsed().as_millis());

    // ── Ensure 3 potion variants on disk + load via OUR crate ──────────
    let potion_paths = [
        ("potion-retrieval-32M",     "minishlab/potion-retrieval-32M"),
        ("potion-code-16M",          "minishlab/potion-code-16M"),
        ("potion-multilingual-128M", "minishlab/potion-multilingual-128M"),
    ];
    let mut potion_encoders: Vec<(&'static str, SaidEncoder)> = Vec::new();
    for (local, hf) in &potion_paths {
        let path = ensure_potion(hf, local)?;
        println!("Loading {} via model2vec-said...", local);
        let t = Instant::now();
        let e = SaidEncoder::from_pretrained(path.to_str().unwrap())?;
        println!("  loaded in {} ms", t.elapsed().as_millis());
        // Leak the &str for static lifetime in the table label.
        potion_encoders.push((Box::leak(local.to_string().into_boxed_str()), e));
    }
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
    let mut contents_prefixed: Vec<String> = Vec::with_capacity(doc_ids.len());
    for id in &doc_ids {
        let body = brain.read(id).unwrap_or_default();
        let trimmed: String = body.chars().take(1500).collect();
        contents_prefixed.push(prefix_with_doc_id(id, &trimmed));
    }
    println!("  read in {} ms", t0.elapsed().as_millis());

    // ── Encode every frame with each encoder ──────────────────────────
    println!("Encoding frames with said-lam-static (prefixed)...");
    let t = Instant::now();
    let said_embs = said_enc.encode_batch(&contents_prefixed);
    let said_dim = said_embs.first().map(|v| v.len()).unwrap_or(0);
    let said_ms = t.elapsed().as_millis();
    println!("  {} ms ({}-dim)", said_ms, said_dim);

    let mut runs: Vec<EncoderRun> = Vec::new();
    runs.push(EncoderRun {
        label: "said-lam-static",
        enc: said_enc,
        frame_embs: said_embs,
        dim: said_dim,
        encode_ms: said_ms,
    });
    for (label, enc) in potion_encoders {
        println!("Encoding frames with {} (prefixed)...", label);
        let t = Instant::now();
        let embs = enc.encode_batch(&contents_prefixed);
        let dim = embs.first().map(|v| v.len()).unwrap_or(0);
        let ms = t.elapsed().as_millis();
        println!("  {} ms ({}-dim)", ms, dim);
        runs.push(EncoderRun { label, enc, frame_embs: embs, dim, encode_ms: ms });
    }
    println!();

    // ── Per-question side-by-side ─────────────────────────────────────
    let n_with_expect = CASES.iter().filter(|c| !c.expect.is_empty()).count();
    let n_with_cross  = CASES.iter().filter(|c| !c.expect_cs.is_empty() && !c.expect_sql.is_empty()).count();
    let mut counts: Vec<(u32, u32, u32)> = vec![(0, 0, 0); runs.len()]; // (found, top3, top5)
    let mut xcounts: Vec<u32> = vec![0; runs.len()]; // cross-language pass count

    println!("{:<5} {:<48} {}",
        "ID", "Question",
        runs.iter().map(|r| format!("{:<10}", r.label.split('-').next().unwrap_or(r.label))).collect::<Vec<_>>().join(""));
    println!("{}", "-".repeat(110));

    let mut report = String::new();
    report.push_str("# Code encoder A/B — code.said\n\n");
    report.push_str(&format!("Brain: `{}` · {} active frames · {} questions · pure cosine, NO LLM\n\n",
        path_said, doc_ids.len(), CASES.len()));
    report.push_str("Encoders (all loaded via `model2vec-said::from_pretrained`):\n\n");
    for r in &runs {
        report.push_str(&format!("- `{}` — {}-dim · encode {} ms / {} frames\n",
            r.label, r.dim, r.encode_ms, doc_ids.len()));
    }
    report.push_str("\n");
    report.push_str("Frames encoded with `[doc:<tokens>] <content>` prefix (proven win for prose; testing here for code).\n\n");
    let mut header_row = String::from("| ID | Question |");
    let mut sep_row    = String::from("|---|---|");
    for r in &runs {
        header_row.push_str(&format!(" {} |", r.label));
        sep_row.push_str("---|");
    }
    report.push_str(&header_row);
    report.push('\n');
    report.push_str(&sep_row);
    report.push('\n');

    for case in CASES {
        let is_cross = !case.expect_cs.is_empty() && !case.expect_sql.is_empty();

        // Encode the query separately per encoder.
        let mut tops: Vec<Vec<(usize, f32)>> = Vec::with_capacity(runs.len());
        let mut cells: Vec<String> = Vec::with_capacity(runs.len());
        for (col, run) in runs.iter().enumerate() {
            let q = run.enc.encode_one(case.question);
            let top = topn(&q, &run.frame_embs, TOP_N);

            let cell = if is_cross {
                // Cross-language: pass = C# match present AND SQL match present in top-N.
                let cs_rank  = find_expected_rank(&top, &doc_ids, case.expect_cs).map(|(r, _)| r);
                let sql_rank = find_expected_rank(&top, &doc_ids, case.expect_sql).map(|(r, _)| r);
                match (cs_rank, sql_rank) {
                    (Some(cr), Some(sr)) => {
                        xcounts[col] += 1;
                        format!("C#{}+SQL{}", cr, sr)
                    }
                    (Some(cr), None) => format!("C#{}/-", cr),
                    (None, Some(sr)) => format!("-/SQL{}", sr),
                    (None, None)     => format!(">{}", TOP_N),
                }
            } else {
                match find_expected_rank(&top, &doc_ids, case.expect) {
                    Some((r, _)) => {
                        counts[col].0 += 1;
                        if r <= 3 { counts[col].1 += 1; }
                        if r <= 5 { counts[col].2 += 1; }
                        format!("rank {}", r)
                    }
                    None if case.expect.is_empty() => "—".to_string(),
                    None => format!(">{}", TOP_N),
                }
            };
            cells.push(cell);
            tops.push(top);
        }

        let q_short = if case.question.len() > 46 {
            format!("{}…", &case.question[..45])
        } else {
            case.question.to_string()
        };
        let cells_str: String = cells.iter().map(|c| format!("{:<10}", c)).collect();
        println!("{:<5} {:<48} {}", case.id, q_short, cells_str);
        let row: String = cells.iter().map(|c| format!(" {} |", c)).collect();
        report.push_str(&format!("| {} | {} |{}\n",
            case.id, case.question.replace('|', r"\|"), row));

        // Per-question per-encoder top-10 detail (markdown only).
        report.push_str(&format!("\n### {} — {}\n\n_{}_\n\n", case.id, case.question, case.note));
        for (run, top) in runs.iter().zip(tops.iter()) {
            report.push_str(&format!("**{}** top-{}:\n\n", run.label, TOP_N));
            for (rank, (idx, sim)) in top.iter().enumerate() {
                let id = &doc_ids[*idx];
                let cs_hit  = case.expect_cs.iter().any(|s| id.contains(s));
                let sql_hit = case.expect_sql.iter().any(|s| id.contains(s));
                let any_hit = case.expect.iter().any(|s| id.contains(s));
                let star = if any_hit { "★ " }
                    else if cs_hit  { "C# " }
                    else if sql_hit { "SQL " }
                    else { "  " };
                report.push_str(&format!("{}{}. `{}` · cos={:.4}\n\n", star, rank + 1, id, sim));
            }
        }
    }
    println!();

    // ── Summary -------------------------------------------------------
    println!("Single-language ({} questions):", n_with_expect);
    for (i, run) in runs.iter().enumerate() {
        println!("  {:<28} found={:>2}/{}  top-3={:>2}/{}  top-5={:>2}/{}",
            run.label, counts[i].0, n_with_expect, counts[i].1, n_with_expect, counts[i].2, n_with_expect);
    }
    println!();
    println!("Cross-language ({} questions, both C# AND SQL must hit top-10):", n_with_cross);
    for (i, run) in runs.iter().enumerate() {
        println!("  {:<28} cross-pass={:>2}/{}", run.label, xcounts[i], n_with_cross);
    }
    report.push_str(&format!("\n## Summary\n\nSingle-language: {} questions · Cross-language: {} questions\n\n",
        n_with_expect, n_with_cross));
    report.push_str("| Encoder | dim | top-10 | top-3 | top-5 | cross C#+SQL | encode latency |\n");
    report.push_str("|---|---|---|---|---|---|---|\n");
    for (i, run) in runs.iter().enumerate() {
        report.push_str(&format!("| `{}` | {} | {} / {} | {} / {} | {} / {} | {} / {} | {} ms |\n",
            run.label, run.dim,
            counts[i].0, n_with_expect,
            counts[i].1, n_with_expect,
            counts[i].2, n_with_expect,
            xcounts[i], n_with_cross,
            run.encode_ms,
        ));
    }

    fs::create_dir_all("research/MinishLab")?;
    fs::write(REPORT_PATH, &report)?;
    println!();
    println!("wrote {}", REPORT_PATH);

    Ok(())
}
