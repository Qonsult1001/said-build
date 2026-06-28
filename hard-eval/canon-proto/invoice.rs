// rendered from canon[Create] -> rust/axum. SAME 6-phase order; 20% = Invoice slots
async fn create_invoice(Json(req): Json<CreateInvoiceReq>) -> impl IntoResponse {   // phase1 accept
    if req.number.is_empty() { return envelope_err("Number is required"); }         // phase3 guard (20%)
    let id = Uuid::new_v4();
    sqlx::query!("INSERT INTO invoice(id,number,amount) VALUES($1,$2,$3)", id, req.number, req.amount).execute(&pool).await?; // phase4 (20%)
    envelope_ok(json!({"id": id, "number": req.number, "amount": req.amount}))      // phase6 project (20%)
}
