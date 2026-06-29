using Microsoft.AspNetCore.Mvc;

namespace Bench.RestCrud;

// REST CRUD example, entity = Invoice. The Create endpoint follows the team's standard shape:
// accept+audit -> idempotency -> guards -> save -> response -> wrap+return. The SAME shape appears in
// OrderController (support>=2) so harvest learns ONE "create" blueprint; only the 20% slots differ here.
[ApiController]
[Route("invoices")]
public class InvoiceController : ControllerBase
{
    private readonly IInvoiceStore _store;
    private readonly IAudit _audit;
    private readonly IIdempotency _idem;

    public InvoiceController(IInvoiceStore store, IAudit audit, IIdempotency idem)
    {
        _store = store; _audit = audit; _idem = idem;
    }

    [HttpPost]
    public async Task<IActionResult> Create([FromBody] CreateInvoiceRequest req)
    {
        // [80%] accept + audit
        var responseId = Guid.NewGuid();
        await _audit.Record(req, responseId);
        // [80%] idempotency
        if (await _idem.Seen(req.IdempotencyKey)) return Conflict();
        await _idem.Mark(req.IdempotencyKey);
        // [20%] guards (entity-specific)
        if (string.IsNullOrWhiteSpace(req.Number)) return BadRequest("Number is required");
        if (req.Amount <= 0) return BadRequest("Amount must be positive");
        // [20%] save (entity-specific)
        var id = Guid.NewGuid();
        await _store.Insert(id, req.Number, req.Amount, req.CustomerId);
        // [20%] response (entity-specific)
        var data = new { id, req.Number, req.Amount, status = "open" };
        // [80%] wrap + return
        var resp = Envelope.Ok(data, responseId);
        await _audit.Complete(responseId, 200, resp);
        return Ok(resp);
    }
}

public record CreateInvoiceRequest(string IdempotencyKey, string Number, decimal Amount, Guid CustomerId);
