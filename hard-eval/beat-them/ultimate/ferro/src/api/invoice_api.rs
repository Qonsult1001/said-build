//! API handlers for invoices. Each handler returns a Result the caller renders.

use crate::models::invoice::{Invoice, NewInvoice};
use crate::service::invoice_service::InvoiceService;

/// POST /invoices — validate the request, persist, return the created row.
pub fn create(svc: &mut InvoiceService, req: NewInvoice) -> Result<Invoice, String> {
    svc.create(req)
}

/// GET /invoices — list all invoices.
pub fn list(svc: &InvoiceService) -> Result<Vec<Invoice>, String> {
    Ok(svc.list())
}

/// GET /invoices/:id — fetch one invoice by id.
pub fn get(svc: &InvoiceService, id: u64) -> Result<Invoice, String> {
    svc.get_by_id(id)
}
