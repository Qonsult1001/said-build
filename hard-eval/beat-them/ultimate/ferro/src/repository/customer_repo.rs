//! In-memory repository for customers.

use crate::models::customer::Customer;
use std::collections::HashMap;

/// Stores customers in a HashMap keyed by id, handing out ids monotonically.
#[derive(Debug, Default)]
pub struct CustomerRepository {
    items: HashMap<u64, Customer>,
    next_id: u64,
}

impl CustomerRepository {
    pub fn new() -> Self {
        // next_id starts at 1 so that id == 0 stays a reserved "unset" sentinel.
        Self { items: HashMap::new(), next_id: 1 }
    }

    /// Persist a customer, assigning it a fresh id. Returns the stored row.
    pub fn insert(&mut self, name: String, email: String) -> Customer {
        let id = self.next_id;
        self.next_id += 1;
        let customer = Customer { id, name, email };
        self.items.insert(id, customer.clone());
        customer
    }

    pub fn list(&self) -> Vec<Customer> {
        let mut rows: Vec<Customer> = self.items.values().cloned().collect();
        rows.sort_by_key(|r| r.id);
        rows
    }

    pub fn get_by_id(&self, id: u64) -> Option<Customer> {
        self.items.get(&id).cloned()
    }
}
