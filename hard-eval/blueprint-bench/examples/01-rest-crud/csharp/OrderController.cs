using Microsoft.AspNetCore.Mvc;

namespace Bench.RestCrud;

// Create Order (dotnet).  Sections: [Sn]..[/Sn] in run order.  Map + how to edit: ../said.index.md
// Tag on each section = can you edit it?   GENERATED (no, .said rewrites it from the blueprint) | YOURS (yes, kept).
// SAME create-shape skeleton as InvoiceController (the GENERATED 80% is identical) -> harvest learns ONE
// `create` blueprint from the two; only the YOURS 20% (Order fields, DML, response) differs.
[ApiController]
[Route("orders")]
public class OrderController : ControllerBase
{
    private readonly IOrderStore _store;
    private readonly IAudit _audit;
    private readonly IIdempotency _idem;

    public OrderController(IOrderStore store, IAudit audit, IIdempotency idem)
    {
        _store = store; _audit = audit; _idem = idem;
    }

    [HttpPost]
    public async Task<IActionResult> Create([FromBody] CreateOrderRequest req)
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
        if (req.Lines is null || req.Lines.Count == 0) return BadRequest("At least one line is required");
        if (req.CustomerId == Guid.Empty) return BadRequest("CustomerId is required");
        // [/S3]

        // [S4] save  YOURS
        var id = Guid.NewGuid();
        await _store.Insert(id, req.CustomerId, req.Lines);
        // [/S4]

        // [S5] response  YOURS
        var data = new { id, req.CustomerId, lineCount = req.Lines.Count, status = "placed" };
        // [/S5]

        // [S6] wrap-and-return  GENERATED
        var resp = Envelope.Ok(data, responseId);
        await _audit.Complete(responseId, 200, resp);
        return Ok(resp);
        // [/S6]
    }
}

public record CreateOrderRequest(string IdempotencyKey, Guid CustomerId, List<OrderLine> Lines);
public record OrderLine(string Sku, int Qty);
