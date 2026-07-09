using Microsoft.AspNetCore.Mvc;

namespace Bench.RestCrud;

// Create Invoice (dotnet).  Sections: [Sn]..[/Sn] in run order.  Map + how to edit: ../said.index.md
// Tag on each section = can you edit it?   GENERATED (no, .said rewrites it from the blueprint) | YOURS (yes, kept).
// The GENERATED sections are the reused 80% (the create-shape skeleton, identical in OrderController);
// the YOURS sections are the entity-specific 20% (Invoice fields, DML, response).
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
        // [S1] accept-and-audit  GENERATED
        var responseId = Guid.NewGuid();
        await _audit.Record(req, responseId);
        // [/S1]

        // [S2] idempotency  GENERATED
        if (await _idem.Seen(req.IdempotencyKey)) return Conflict();
        await _idem.Mark(req.IdempotencyKey);
        // [/S2]

        // [S3] guards  YOURS
        if (string.IsNullOrWhiteSpace(req.Number)) return BadRequest("Number is required");
        if (req.Amount <= 0) return BadRequest("Amount must be positive");
        // [/S3]

        // [S4] save  YOURS
        var id = Guid.NewGuid();
        await _store.Insert(id, req.Number, req.Amount, req.CustomerId);
        // [/S4]

        // [S5] response  YOURS
        var data = new { id, req.Number, req.Amount, status = "open" };
        // [/S5]

        // [S6] wrap-and-return  GENERATED
        var resp = Envelope.Ok(data, responseId);
        await _audit.Complete(responseId, 200, resp);
        return Ok(resp);
        // [/S6]
    }
}

public record CreateInvoiceRequest(string IdempotencyKey, string Number, decimal Amount, Guid CustomerId);
