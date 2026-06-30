//! Orchestration service for payments: wires validation -> repository -> response.

use crate::models::payment::{NewPayment, Payment};
use crate::repository::payment_repo::PaymentRepository;

/// Thin service that owns a payment repository and enforces the create flow.
#[derive(Debug, Default)]
pub struct PaymentService {
    repo: PaymentRepository,
}

impl PaymentService {
    pub fn new() -> Self {
        Self { repo: PaymentRepository::new() }
    }

    /// create = validate the request -> persist to repository -> return the response.
    pub fn create(&mut self, req: NewPayment) -> Result<Payment, String> {
        req.validate()?;
        let stored = self.repo.insert(req.invoice_id, req.amount_cents, req.method);
        Ok(stored)
    }

    pub fn list(&self) -> Vec<Payment> {
        self.repo.list()
    }

    pub fn get_by_id(&self, id: u64) -> Result<Payment, String> {
        self.repo
            .get_by_id(id)
            .ok_or_else(|| format!("payment {id} not found"))
    }
}
