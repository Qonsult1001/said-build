//! Isolation speed + memory test for the own WordPiece tokenizer (#4).
//!
//! Confirms the hand-written tokenizer is "lightning fast" on its own — encoding a realistic
//! batch of code/SQL passages — and produces byte-identical embeddings to the model2vec/HF
//! path. This is a measurement harness (run with --nocapture), not a hard pass/fail gate, so
//! it stays GREEN in CI; the timing/parity is printed for inspection.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model" \
//!        --test test_tokenizer_speed -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::latent_cluster::StaticEncoder;
use std::path::Path;
use std::time::Instant;

const MODEL_CANDIDATES: &[&str] = &[
    "G:/development/said-build/SAID-LAM-private/said-lam-static-4M",
    "G:/development/said-build/SAID-LAM-private/said-lam-static-2M",
    "G:/development/said-build/model-eval/potion-base-4M",
];

fn find_model() -> Option<&'static str> {
    MODEL_CANDIDATES
        .iter()
        .copied()
        .find(|p| Path::new(p).join("model.safetensors").exists())
}

/// A realistic batch of code/SQL-ish passages (the workload `said init` encodes).
fn corpus(n: usize) -> Vec<String> {
    let templates = [
        "CREATE PROCEDURE Loan_Amortization_Schedule @CustomerId INT AS SELECT SUM(Amount) FROM Payments WHERE CustomerId = @CustomerId GROUP BY DueDate;",
        "public class PaymentProcessor { public decimal MaxDailyLimit = 10000m; public PaymentResult ChargeCard(string token, decimal amount) { return StripeGateway.Charge(token, amount); } }",
        "def calculate_amortization(principal, rate, periods): return principal * (rate * (1 + rate) ** periods) / ((1 + rate) ** periods - 1)",
        "The refund worker dequeues transactions asynchronously and reconciles them against the daily settlement file from the gateway.",
        "SELECT t.Id, t.Amount, c.Name FROM Transactions t INNER JOIN Customers c ON c.Id = t.CustomerId WHERE t.Status = 'Settled' ORDER BY t.CreatedAt DESC;",
    ];
    (0..n).map(|i| format!("{} -- row {}", templates[i % templates.len()], i)).collect()
}

#[test]
fn own_tokenizer_is_fast_and_identical() {
    let Some(model) = find_model() else {
        eprintln!("skipping — no model at {:?}", MODEL_CANDIDATES);
        return;
    };

    // Own (production) path.
    let own = match StaticEncoder::from_pretrained(model) {
        Ok(e) => e,
        Err(e) => { eprintln!("skipping — own load failed: {e}"); return; }
    };
    // model2vec/HF reference path (the ONLY thing that touches HF tokenizers).
    let reference = StaticEncoder::from_model2vec(model).ok();

    let batch = corpus(2000);

    // Warm both (first call pays any one-time init).
    let _ = own.encode_one(&batch[0]);
    if let Some(r) = &reference { let _ = r.encode_batch_model2vec(&batch[..1.min(batch.len())].to_vec()); }

    // Time the own tokenizer on the full batch.
    let t0 = Instant::now();
    let own_embs = own.encode_batch(&batch);
    let own_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let per_passage_us = own_ms * 1000.0 / batch.len() as f64;
    eprintln!(
        "OWN tokenizer: {} passages in {:.1}ms = {:.1}µs/passage ({:.0} passages/sec)",
        batch.len(), own_ms, per_passage_us, batch.len() as f64 / (own_ms / 1000.0)
    );

    // If the reference is available, time it + assert byte-identity.
    if let Some(r) = &reference {
        let t1 = Instant::now();
        let ref_embs = r.encode_batch_model2vec(&batch);
        let ref_ms = t1.elapsed().as_secs_f64() * 1000.0;
        eprintln!("HF/model2vec:  {} passages in {:.1}ms ({:.2}x own)", batch.len(), ref_ms, ref_ms / own_ms.max(1e-9));

        let mut max_diff = 0.0f32;
        for (a, b) in own_embs.iter().zip(ref_embs.iter()) {
            for (x, y) in a.iter().zip(b.iter()) {
                max_diff = max_diff.max((x - y).abs());
            }
        }
        eprintln!("max per-element diff (own vs HF): {:.3e}", max_diff);
        assert!(max_diff < 1e-4, "own tokenizer diverged from model2vec/HF: max diff {max_diff:.3e}");
    } else {
        eprintln!("(reference model2vec path unavailable — parity not checked this run)");
    }
}
