// Create Invoice (rust/axum). Sections: [Sn]..[/Sn] in run order. Map + how to edit: said.index.md
// Tag = can you edit it?  GENERATED (no, .said rewrites it) | YOURS (yes, kept).
async fn create_invoice(
    State(app): State<AppState>,
    Json(req): Json<CreateInvoiceReq>,
) -> impl IntoResponse {
    // [S1] accept-and-audit  GENERATED
    let response_id = Uuid::new_v4();
    app.audit.insert(&req, response_id).await;
    // [/S1]

    // [S2] idempotency  GENERATED
    if app.seen.contains(&req.idempotency_key) { return conflict(); }
    app.seen.insert(req.idempotency_key.clone());
    // [/S2]

    // [S3] guards  YOURS
    if req.number.is_empty() { return envelope_err("Number is required"); }
    // [/S3]

    // [S4] save  YOURS
    let id = Uuid::new_v4();
    sqlx::query!("INSERT INTO invoice(id,number,amount) VALUES($1,$2,$3)",
        id, req.number, req.amount).execute(&app.pool).await?;
    // [/S4]

    // [S5] response  YOURS
    let data = json!({ "id": id, "number": req.number, "amount": req.amount });
    // [/S5]

    // [S6] wrap+return  GENERATED
    let resp = envelope_ok(data, response_id);
    app.audit.update(response_id, 200, &resp).await;
    Json(resp)
    // [/S6]
}
