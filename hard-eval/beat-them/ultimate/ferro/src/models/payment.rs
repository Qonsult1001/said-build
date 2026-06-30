//! Payment domain model.
//! Rendered from the recalled "Create<Entity> endpoint" blueprint
//! (validate the request -> persist to repository -> return the response).

/// A payment applied against an invoice.
#[derive(Debug, Clone, PartialEq)]
pub struct Payment {
    pub id: u64,
    pub invoice_id: u64,
    pub amount_cents: i64,
    pub method: String,
}

/// The fields a caller supplies to create a payment (no id yet).
#[derive(Debug, Clone)]
pub struct NewPayment {
    pub invoice_id: u64,
    pub amount_cents: i64,
    pub method: String,
}

impl NewPayment {
    /// Validate the request before it is persisted.
    ///
    /// Invariant: a payment must reference a real invoice, settle a strictly
    /// positive amount, and name a payment method — we never persist a payment
    /// that can't be reconciled.
    pub fn validate(&self) -> Result<(), String> {
        if self.invoice_id == 0 {
            return Err("payment.invoice_id must be non-zero".to_string());
        }
        if self.amount_cents <= 0 {
            return Err("payment.amount_cents must be positive".to_string());
        }
        if self.method.trim().is_empty() {
            return Err("payment.method must not be empty".to_string());
        }
        Ok(())
    }
}
