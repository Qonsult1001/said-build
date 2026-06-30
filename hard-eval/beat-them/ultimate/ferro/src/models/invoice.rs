//! Invoice domain model.

/// An invoice issued to a customer for a given amount.
#[derive(Debug, Clone, PartialEq)]
pub struct Invoice {
    pub id: u64,
    pub customer_id: u64,
    pub amount_cents: i64,
    pub memo: String,
}

/// The fields a caller supplies to create an invoice (no id yet).
#[derive(Debug, Clone)]
pub struct NewInvoice {
    pub customer_id: u64,
    pub amount_cents: i64,
    pub memo: String,
}

impl NewInvoice {
    /// Validate the request before it is persisted.
    ///
    /// Invariant: an invoice must reference a real customer and carry a
    /// strictly positive amount — a zero/negative invoice is meaningless.
    pub fn validate(&self) -> Result<(), String> {
        if self.customer_id == 0 {
            return Err("invoice.customer_id must be non-zero".to_string());
        }
        if self.amount_cents <= 0 {
            return Err("invoice.amount_cents must be positive".to_string());
        }
        Ok(())
    }
}
