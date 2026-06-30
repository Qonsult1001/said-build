//! In-memory repository for payments.

use crate::models::payment::Payment;
use std::collections::HashMap;

/// Stores payments in a HashMap keyed by id, handing out ids monotonically.
#[derive(Debug, Default)]
pub struct PaymentRepository {
    items: HashMap<u64, Payment>,
    next_id: u64,
}

impl PaymentRepository {
    pub fn new() -> Self {
        // next_id starts at 1 so that id == 0 stays a reserved "unset" sentinel.
        Self { items: HashMap::new(), next_id: 1 }
    }

    /// Persist a payment, assigning it a fresh id. Returns the stored row.
    pub fn insert(&mut self, invoice_id: u64, amount_cents: i64, method: String) -> Payment {
        let id = self.next_id;
        self.next_id += 1;
        let payment = Payment { id, invoice_id, amount_cents, method };
        self.items.insert(id, payment.clone());
        payment
    }

    pub fn list(&self) -> Vec<Payment> {
        let mut rows: Vec<Payment> = self.items.values().cloned().collect();
        rows.sort_by_key(|r| r.id);
        rows
    }

    pub fn get_by_id(&self, id: u64) -> Option<Payment> {
        self.items.get(&id).cloned()
    }
}
