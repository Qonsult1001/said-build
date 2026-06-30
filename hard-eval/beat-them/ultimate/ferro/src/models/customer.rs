//! Customer domain model.
//! Rendered from the recalled "Create<Entity> endpoint" blueprint
//! (validate the request -> persist to repository -> return the response).

/// A customer who can be invoiced.
#[derive(Debug, Clone, PartialEq)]
pub struct Customer {
    pub id: u64,
    pub name: String,
    pub email: String,
}

/// The fields a caller supplies to create a customer (no id yet).
#[derive(Debug, Clone)]
pub struct NewCustomer {
    pub name: String,
    pub email: String,
}

impl NewCustomer {
    /// Validate the request before it is persisted.
    ///
    /// Invariant: a customer needs a non-empty name and an email that at least
    /// looks addressable (`contains('@')`) — we never persist a nameless or
    /// unreachable customer.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("customer.name must not be empty".to_string());
        }
        if !self.email.contains('@') {
            return Err("customer.email must contain '@'".to_string());
        }
        Ok(())
    }
}
