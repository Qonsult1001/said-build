//! API handlers for customers. Each handler returns a Result the caller renders.

use crate::models::customer::{Customer, NewCustomer};
use crate::service::customer_service::CustomerService;

/// POST /customers — validate the request, persist, return the created row.
pub fn create(svc: &mut CustomerService, req: NewCustomer) -> Result<Customer, String> {
    svc.create(req)
}

/// GET /customers — list all customers.
pub fn list(svc: &CustomerService) -> Result<Vec<Customer>, String> {
    Ok(svc.list())
}

/// GET /customers/:id — fetch one customer by id.
pub fn get(svc: &CustomerService, id: u64) -> Result<Customer, String> {
    svc.get_by_id(id)
}
