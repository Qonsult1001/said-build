// RENDERED from the recalled `create<Entity>` blueprint (harvested from the C# InvoiceController/OrderController).
// The agent did NOT re-derive the structure: the [80%] skeleton came from .said's recalled blueprint
// (NewGuid -> Record -> Seen -> Conflict -> Mark -> guards -> BadRequest -> save -> Insert -> response ->
// Complete), rendered in rust/axum. Only the [20%] slots (Invoice fields, guards, DML, response) were written.
use axum::{extract::State, http::StatusCode, Json};
use uuid::Uuid;

pub async fn create_invoice(
    State(app): State<AppState>,
    Json(req): Json<CreateInvoiceRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // [80%] accept + audit            (blueprint: NewGuid, Record)
    let response_id = Uuid::new_v4();
    app.audit.record(&req, response_id).await;
    // [80%] idempotency               (blueprint: Seen, Conflict, Mark)
    if app.idem.seen(&req.idempotency_key).await {
        return Err((StatusCode::CONFLICT, "duplicate request".into()));
    }
    app.idem.mark(&req.idempotency_key).await;
    // [20%] guards (entity-specific)
    if req.number.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Number is required".into()));
    }
    if req.amount <= 0.0 {
        return Err((StatusCode::BAD_REQUEST, "Amount must be positive".into()));
    }
    // [20%] save (entity-specific)
    let id = Uuid::new_v4();
    app.store.insert(id, &req.number, req.amount, req.customer_id).await;
    // [20%] response (entity-specific)
    let data = serde_json::json!({ "id": id, "number": req.number, "amount": req.amount, "status": "open" });
    // [80%] wrap + return             (blueprint: response, Complete)
    let resp = envelope_ok(data, response_id);
    app.audit.complete(response_id, 200, &resp).await;
    Ok(Json(resp))
}

#[derive(serde::Deserialize)]
pub struct CreateInvoiceRequest {
    pub idempotency_key: String,
    pub number: String,
    pub amount: f64,
    pub customer_id: Uuid,
}
