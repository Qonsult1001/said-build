//! In-memory repository for invoices.

use crate::models::invoice::Invoice;
use std::collections::HashMap;

/// Stores invoices in a HashMap keyed by id, handing out ids monotonically.
#[derive(Debug, Default)]
pub struct InvoiceRepository {
    items: HashMap<u64, Invoice>,
    next_id: u64,
}

impl InvoiceRepository {
    pub fn new() -> Self {
        // next_id starts at 1 so that id == 0 stays a reserved "unset" sentinel.
        Self { items: HashMap::new(), next_id: 1 }
    }

    /// Persist an invoice, assigning it a fresh id. Returns the stored row.
    pub fn insert(&mut self, customer_id: u64, amount_cents: i64, memo: String) -> Invoice {
        let id = self.next_id;
        self.next_id += 1;
        let invoice = Invoice { id, customer_id, amount_cents, memo };
        self.items.insert(id, invoice.clone());
        invoice
    }

    pub fn list(&self) -> Vec<Invoice> {
        let mut rows: Vec<Invoice> = self.items.values().cloned().collect();
        rows.sort_by_key(|r| r.id);
        rows
    }

    pub fn get_by_id(&self, id: u64) -> Option<Invoice> {
        self.items.get(&id).cloned()
    }
}
