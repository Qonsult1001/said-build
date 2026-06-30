//! Orchestration service for customers: wires validation -> repository -> response.

use crate::models::customer::{Customer, NewCustomer};
use crate::repository::customer_repo::CustomerRepository;

/// Thin service that owns a customer repository and enforces the create flow.
#[derive(Debug, Default)]
pub struct CustomerService {
    repo: CustomerRepository,
}

impl CustomerService {
    pub fn new() -> Self {
        Self { repo: CustomerRepository::new() }
    }

    /// create = validate the request -> persist to repository -> return the response.
    pub fn create(&mut self, req: NewCustomer) -> Result<Customer, String> {
        req.validate()?;
        let stored = self.repo.insert(req.name, req.email);
        Ok(stored)
    }

    pub fn list(&self) -> Vec<Customer> {
        self.repo.list()
    }

    pub fn get_by_id(&self, id: u64) -> Result<Customer, String> {
        self.repo
            .get_by_id(id)
            .ok_or_else(|| format!("customer {id} not found"))
    }
}
