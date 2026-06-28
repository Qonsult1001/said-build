// @said-managed: Fully  region=header
// =============================================
// Endpoint:    POST /invoice   (Create<Entity>)
// Rendered from canon[Create] -> dotnet/flat. The Fully regions are framework-owned (regenerated
// byte-identical); the Ignore slots are the 20% the agent fills from the Invoice schema.
// =============================================
// @said-managed: end

[ApiController]
[Route("invoice")]
public class InvoiceController : ControllerBase
{
    private readonly IDb _db;
    public InvoiceController(IDb db) { _db = db; }

    [HttpPost]
    public IActionResult Create([FromBody] CreateInvoiceRequest req)
    {
        // @said-managed: Fully  region=accept-and-audit
        var responseId = Guid.NewGuid();
        _audit.Insert(req, responseId);              // framework: request audit row
        // @said-managed: end

        // @said-managed: Ignore  slot=1_guards
        if (string.IsNullOrEmpty(req.Number))
            return BadRequest(Envelope.Error("Number is required"));
        // @said-managed: end

        // @said-managed: Ignore  slot=2_dml
        var id = Guid.NewGuid();
        _db.Execute(
            "INSERT INTO Invoice(Id, Number, Amount) VALUES(@id, @n, @a)",
            new { id, n = req.Number, a = req.Amount });
        // @said-managed: end

        // @said-managed: Ignore  slot=3_response
        var data = new { Id = id, req.Number, req.Amount };
        // @said-managed: end

        // @said-managed: Fully  region=envelope-wrap-and-return
        var resp = Envelope.Ok(data, responseId);    // framework: response envelope
        _audit.Update(responseId, 200, resp);        // framework: response audit update
        return Ok(resp);
        // @said-managed: end
    }
}
