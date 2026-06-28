// @said-managed: Fully  region=header
// =============================================
// Endpoint:    POST /invoice   (Create<Entity>)
// Rendered from canon[Create] -> rust/axum. SAME contract: Fully = framework-owned (regenerated),
// Ignore slots = the 20% the agent fills from the Invoice schema. Same phase order as the dotnet render.
// =============================================
// @said-managed: end

async fn create_invoice(
    State(app): State<AppState>,
    Json(req): Json<CreateInvoiceReq>,
) -> impl IntoResponse {
    // @said-managed: Fully  region=accept-and-audit
    let response_id = Uuid::new_v4();
    app.audit.insert(&req, response_id).await;        // framework: request audit row
    // @said-managed: end

    // @said-managed: Ignore  slot=1_guards
    if req.number.is_empty() {
        return envelope_err("Number is required");
    }
    // @said-managed: end

    // @said-managed: Ignore  slot=2_dml
    let id = Uuid::new_v4();
    sqlx::query!(
        "INSERT INTO invoice(id, number, amount) VALUES($1, $2, $3)",
        id, req.number, req.amount
    ).execute(&app.pool).await?;
    // @said-managed: end

    // @said-managed: Ignore  slot=3_response
    let data = json!({ "id": id, "number": req.number, "amount": req.amount });
    // @said-managed: end

    // @said-managed: Fully  region=envelope-wrap-and-return
    let resp = envelope_ok(data, response_id);        // framework: response envelope
    app.audit.update(response_id, 200, &resp).await;  // framework: response audit update
    Json(resp)
    // @said-managed: end
}
