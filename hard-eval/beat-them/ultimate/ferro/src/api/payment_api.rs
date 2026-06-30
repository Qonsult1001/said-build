//! API handlers for payments. Each handler returns a Result the caller renders.

use crate::models::payment::{NewPayment, Payment};
use crate::service::payment_service::PaymentService;

/// POST /payments — validate the request, persist, return the created row.
pub fn create(svc: &mut PaymentService, req: NewPayment) -> Result<Payment, String> {
    svc.create(req)
}

/// GET /payments — list all payments.
pub fn list(svc: &PaymentService) -> Result<Vec<Payment>, String> {
    Ok(svc.list())
}

/// GET /payments/:id — fetch one payment by id.
pub fn get(svc: &PaymentService, id: u64) -> Result<Payment, String> {
    svc.get_by_id(id)
}
