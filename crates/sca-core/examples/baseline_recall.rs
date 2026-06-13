//! Recall benchmark — runs every Section 1 question through the full
//! production `ask::ask` pipeline (SCA → grep → ABC fusion → recall_floor)
//! and reports per-question pass/fail. This is the canonical regression
//! check we re-run after every change to the recall pipeline.
//!
//! No A/B flags any more — the floor was promoted into `ask::ask` itself
//! after the previous A/B run proved a net +2 improvement. Future
//! improvements (temporal layer, graph extractor, etc.) should land in
//! the same place and re-run THIS harness for regression-proofing.
//!
//! Run:
//!   PATH_SAID=willie.said
//!   cargo run --release --example baseline_recall --features static-embed
//!
//! Output: stdout summary + research/MinishLab/baseline_results.md.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use sca_core::ask::{ask, AskCandidate};
use sca_core::said_file::SaidFile;

const TOP_K: usize = 20;
const REPORT_PATH: &str = "research/MinishLab/baseline_results.md";

#[derive(Debug)]
struct TestCase {
    id: &'static str,
    question: &'static str,
    note: &'static str,
    /// Doc-id substrings — any one of these in top-3 = pass.
    expect_in_top3: &'static [&'static str],
    /// Doc-id substrings — any one of these in top-5 = pass.
    expect_in_top5: &'static [&'static str],
    /// Substrings that should NOT dominate top-3 (negative test).
    should_not_top3: &'static [&'static str],
}

const CASES: &[TestCase] = &[
    TestCase {
        id: "S2.1",
        question: "What is the Tier 1 financial threshold?",
        note: "Above $25,000,000",
        expect_in_top3: &["Bylaws_v7"],
        expect_in_top5: &["Bylaws_v7"],
        should_not_top3: &[],
    },
    TestCase {
        id: "S2.2",
        question: "Who signed the Vendor SLA for Project Prometheus?",
        note: "Brandt + Hanni, June 3 2024",
        expect_in_top3: &["Vendor_SLA_Prometheus"],
        expect_in_top5: &["Vendor_SLA_Prometheus"],
        should_not_top3: &[],
    },
    TestCase {
        id: "S2.3",
        question: "When was Zephyr Holdings Ltd incorporated?",
        note: "12 February 2024",
        expect_in_top3: &["Companies_House_Search_Zephyr"],
        expect_in_top5: &["Companies_House_Search_Zephyr"],
        should_not_top3: &[],
    },
    TestCase {
        id: "S2.4",
        question: "What date was the Emergency Board Minutes meeting?",
        note: "May 14, 2024",
        expect_in_top3: &["Emergency_Board_Minutes"],
        expect_in_top5: &["Emergency_Board_Minutes"],
        should_not_top3: &[],
    },
    TestCase {
        id: "1.1",
        question: "What happened with R.C. Hanni in May 2024?",
        note: "May docs (Slack, Emergency Board), not June Vendor SLA",
        expect_in_top3: &["Slack_Export_IT_May2024", "Emergency_Board_Minutes_May"],
        expect_in_top5: &["Slack_Export_IT_May2024", "Emergency_Board_Minutes_May"],
        should_not_top3: &["Vendor_SLA_Prometheus_June"],
    },
    TestCase {
        id: "1.2",
        question: "What happened with R.C. Hanni in June 2024?",
        note: "Should flip to June Vendor SLA. May Slack should drop.",
        expect_in_top3: &["Vendor_SLA_Prometheus_June"],
        expect_in_top5: &["Vendor_SLA_Prometheus_June"],
        should_not_top3: &["Slack_Export_IT_May2024"],
    },
    TestCase {
        id: "1.3",
        question: "What is the most recent thing about Project Prometheus?",
        note: "'most recent' should bias to chronologically latest",
        expect_in_top3: &["Vendor_SLA_Prometheus_June"],
        expect_in_top5: &["Vendor_SLA_Prometheus_June"],
        should_not_top3: &["Board_Minutes_Q1_2024", "Board_Minutes_March"],
    },
    TestCase {
        id: "1.4",
        question: "What was discussed about Hanni earlier this year?",
        note: "Vague temporal — Hanni-related, recency-first",
        expect_in_top3: &["Hanni", "Zephyr", "Vendor_SLA", "Slack_Export"],
        expect_in_top5: &["Hanni", "Zephyr", "Vendor_SLA", "Slack_Export"],
        should_not_top3: &[],
    },
    TestCase {
        id: "1.5",
        question: "Tell me about events from before May 2024",
        note: "'before X' — Q1 board minutes / Bylaws should rank up",
        expect_in_top3: &["Board_Minutes_Q1", "Bylaws_v7", "Companies_House"],
        expect_in_top5: &["Board_Minutes_Q1", "Bylaws_v7", "Companies_House"],
        should_not_top3: &["Slack_Export_IT_May", "Emergency_Board_Minutes_May", "Vendor_SLA_Prometheus_June"],
    },
    TestCase {
        id: "1.6",
        question: "What happened in 1998?",
        note: "Out-of-corpus year — only Dogman 1998-02-01 RC Hanni line",
        expect_in_top3: &["Dogman", "JHW065", "1998"],
        expect_in_top5: &["Dogman", "JHW065", "1998"],
        should_not_top3: &[],
    },
    TestCase {
        id: "1.7",
        question: "What happened on 14 May 2024 specifically?",
        note: "Exact-date — Slack_Export May 14 should be #1",
        expect_in_top3: &["Slack_Export_IT_May2024", "Emergency_Board_Minutes_May"],
        expect_in_top5: &["Slack_Export_IT_May2024", "Emergency_Board_Minutes_May"],
        should_not_top3: &["Vendor_SLA_Prometheus_June"],
    },
    TestCase {
        id: "1.8",
        question: "Show me events from yesterday",
        note: "Date with zero evidence — engine should NOT confabulate",
        expect_in_top3: &[],
        expect_in_top5: &[],
        should_not_top3: &[],
    },
];

#[derive(Debug, Clone)]
struct CaseResult {
    elapsed_ms: u128,
    candidate_count: usize,
    top5: Vec<(String, f32, String)>,
    expect_top3_pass: Option<bool>,
    expect_top5_pass: Option<bool>,
    should_not_top3_pass: Option<bool>,
}

fn matches_any(doc_id: &str, substrs: &[&str]) -> bool {
    substrs.iter().any(|s| doc_id.contains(s))
}

fn run_case(brain: &mut SaidFile, case: &TestCase) -> CaseResult {
    let t0 = Instant::now();
    let (cands, _kw) = ask(brain, case.question, TOP_K, false, None);
    let elapsed_ms = t0.elapsed().as_millis();

    let top5: Vec<(String, f32, String)> = cands
        .iter()
        .take(5)
        .map(|c: &AskCandidate| (c.doc_id.clone(), c.confidence, c.kind.to_string()))
        .collect();

    let expect_top3_pass = if case.expect_in_top3.is_empty() {
        None
    } else {
        Some(top5.iter().take(3).any(|(id, _, _)| matches_any(id, case.expect_in_top3)))
    };
    let expect_top5_pass = if case.expect_in_top5.is_empty() {
        None
    } else {
        Some(top5.iter().any(|(id, _, _)| matches_any(id, case.expect_in_top5)))
    };
    let should_not_top3_pass = if case.should_not_top3.is_empty() {
        None
    } else {
        Some(!top5.iter().take(3).any(|(id, _, _)| matches_any(id, case.should_not_top3)))
    };

    CaseResult {
        elapsed_ms,
        candidate_count: cands.len(),
        top5,
        expect_top3_pass,
        expect_top5_pass,
        should_not_top3_pass,
    }
}

fn glyph(o: Option<bool>) -> &'static str {
    match o {
        Some(true) => "✅",
        Some(false) => "❌",
        None => "—",
    }
}

fn score(r: &CaseResult) -> u8 {
    let mut s = 0u8;
    if r.expect_top3_pass == Some(true) { s += 1; }
    if r.expect_top5_pass == Some(true) { s += 1; }
    if r.should_not_top3_pass == Some(true) { s += 1; }
    s
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path_said = env::var("PATH_SAID").unwrap_or_else(|_| "willie.said".into());
    println!("=== recall benchmark (production ask::ask path) ===");
    println!("brain: {}", path_said);
    println!("top-K: {}", TOP_K);
    println!("cases: {}\n", CASES.len());

    let mut brain = SaidFile::open(PathBuf::from(&path_said))?;

    let mut results: Vec<CaseResult> = Vec::with_capacity(CASES.len());
    for case in CASES {
        results.push(run_case(&mut brain, case));
    }

    println!("{:<6} {:<54} {:<5} {:<5} {:<5} {:<8}",
        "ID", "Question", "top3", "top5", "neg", "latency");
    println!("{}", "-".repeat(90));
    for (case, r) in CASES.iter().zip(results.iter()) {
        let q = if case.question.len() > 52 {
            format!("{}…", &case.question[..51])
        } else {
            case.question.to_string()
        };
        println!("{:<6} {:<54} {:<5} {:<5} {:<5} {} ms",
            case.id, q,
            glyph(r.expect_top3_pass),
            glyph(r.expect_top5_pass),
            glyph(r.should_not_top3_pass),
            r.elapsed_ms,
        );
    }
    println!();

    let n = CASES.len();
    let pos3 = results.iter().filter(|r| r.expect_top3_pass == Some(true)).count();
    let pos3_n = results.iter().filter(|r| r.expect_top3_pass.is_some()).count();
    let pos5 = results.iter().filter(|r| r.expect_top5_pass == Some(true)).count();
    let pos5_n = results.iter().filter(|r| r.expect_top5_pass.is_some()).count();
    let neg = results.iter().filter(|r| r.should_not_top3_pass == Some(true)).count();
    let neg_n = results.iter().filter(|r| r.should_not_top3_pass.is_some()).count();
    let total: u32 = results.iter().map(|r| score(r) as u32).sum();
    let avg_ms: u128 = results.iter().map(|r| r.elapsed_ms).sum::<u128>() / n as u128;

    println!("Totals:");
    println!("  top-3 expect:   {} / {} ({:.0}%)", pos3, pos3_n, 100.0 * pos3 as f32 / pos3_n.max(1) as f32);
    println!("  top-5 expect:   {} / {} ({:.0}%)", pos5, pos5_n, 100.0 * pos5 as f32 / pos5_n.max(1) as f32);
    println!("  no-violator:    {} / {} ({:.0}%)", neg, neg_n, 100.0 * neg as f32 / neg_n.max(1) as f32);
    println!("  total points:   {}", total);
    println!("  avg latency:    {} ms", avg_ms);

    Ok(())
}
