//! Orchestration service for invoices: wires validation -> repository -> response.

use crate::models::invoice::{Invoice, NewInvoice};
use crate::repository::invoice_repo::InvoiceRepository;

/// Thin service that owns an invoice repository and enforces the create flow.
#[derive(Debug, Default)]
pub struct InvoiceService {
    repo: InvoiceRepository,
}

impl InvoiceService {
    pub fn new() -> Self {
        Self { repo: InvoiceRepository::new() }
    }

    /// create = validate the request -> persist to repository -> return the response.
    pub fn create(&mut self, req: NewInvoice) -> Result<Invoice, String> {
        req.validate()?;
        let stored = self.repo.insert(req.customer_id, req.amount_cents, req.memo);
        Ok(stored)
    }

    pub fn list(&self) -> Vec<Invoice> {
        self.repo.list()
    }

    pub fn get_by_id(&self, id: u64) -> Result<Invoice, String> {
        self.repo
            .get_by_id(id)
            .ok_or_else(|| format!("invoice {id} not found"))
    }
}
