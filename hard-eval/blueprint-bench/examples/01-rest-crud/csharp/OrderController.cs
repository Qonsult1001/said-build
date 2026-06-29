using Microsoft.AspNetCore.Mvc;

namespace Bench.RestCrud;

// SAME create shape as InvoiceController (the 80% skeleton is byte-identical in structure); only the 20%
// slots differ (Order fields, DML, response). Two occurrences => harvest learns ONE "create" blueprint.
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
        // [80%] accept + audit
        var responseId = Guid.NewGuid();
        await _audit.Record(req, responseId);
        // [80%] idempotency
        if (await _idem.Seen(req.IdempotencyKey)) return Conflict();
        await _idem.Mark(req.IdempotencyKey);
        // [20%] guards (entity-specific)
        if (req.Lines is null || req.Lines.Count == 0) return BadRequest("At least one line is required");
        if (req.CustomerId == Guid.Empty) return BadRequest("CustomerId is required");
        // [20%] save (entity-specific)
        var id = Guid.NewGuid();
        await _store.Insert(id, req.CustomerId, req.Lines);
        // [20%] response (entity-specific)
        var data = new { id, req.CustomerId, lineCount = req.Lines.Count, status = "placed" };
        // [80%] wrap + return
        var resp = Envelope.Ok(data, responseId);
        await _audit.Complete(responseId, 200, resp);
        return Ok(resp);
    }
}

public record CreateOrderRequest(string IdempotencyKey, Guid CustomerId, List<OrderLine> Lines);
public record OrderLine(string Sku, int Qty);
