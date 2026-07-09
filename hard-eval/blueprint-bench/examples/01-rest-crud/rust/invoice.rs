// Create Invoice (rust/axum).  Sections: [Sn]..[/Sn] in run order.  Map + how to edit: said.index.md
// Tag on each section = can you edit it?   GENERATED (no, .said rewrites it from the blueprint) | YOURS (yes, kept).
// Rendered cross-language from the recalled `create` blueprint (harvested from the C# controllers); the
// GENERATED sections ARE the reused 80% skeleton, the YOURS sections are the entity-specific 20%.
use axum::{extract::State, http::StatusCode, Json};
use uuid::Uuid;

pub async fn create_invoice(
    State(app): State<AppState>,
    Json(req): Json<CreateInvoiceRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // [S1] accept-and-audit  GENERATED
    let response_id = Uuid::new_v4();
    app.audit.record(&req, response_id).await;
    // [/S1]

    // [S2] idempotency  GENERATED
    if app.idem.seen(&req.idempotency_key).await {
        return Err((StatusCode::CONFLICT, "duplicate request".into()));
    }
    app.idem.mark(&req.idempotency_key).await;
    // [/S2]

    // [S3] guards  YOURS
    if req.number.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Number is required".into()));
    }
    if req.amount <= 0.0 {
        return Err((StatusCode::BAD_REQUEST, "Amount must be positive".into()));
    }
    // [/S3]

    // [S4] save  YOURS
    let id = Uuid::new_v4();
    app.store.insert(id, &req.number, req.amount, req.customer_id).await;
    // [/S4]

    // [S5] response  YOURS
    let data = serde_json::json!({ "id": id, "number": req.number, "amount": req.amount, "status": "open" });
    // [/S5]

    // [S6] wrap+return  GENERATED
    let resp = envelope_ok(data, response_id);
    app.audit.complete(response_id, 200, &resp).await;
    Ok(Json(resp))
    // [/S6]
}

#[derive(serde::Deserialize)]
pub struct CreateInvoiceRequest {
    pub idempotency_key: String,
    pub number: String,
    pub amount: f64,
    pub customer_id: Uuid,
}
