// @said-managed: index  canon=Create<Entity>  lang=dotnet  entity=Invoice
//  §1 accept-and-audit   Fully    framework
//  §2 guards             Ignore   entity-20%
//  §3 dml                Ignore   entity-20%
//  §4 response           Ignore   entity-20%
//  §5 envelope+return    Fully    framework
//  §6 idempotency-key    AI       net-new (not in canon — AI added)
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
        // §1
        var responseId = Guid.NewGuid();
        _audit.Insert(req, responseId);
        // §end

        // §6 AI
        if (_seen.Contains(req.IdempotencyKey)) return Conflict();
        _seen.Add(req.IdempotencyKey);
        // §end

        // §2
        if (string.IsNullOrEmpty(req.Number))
            return BadRequest(Envelope.Error("Number is required"));
        // §end

        // §3
        var id = Guid.NewGuid();
        _db.Execute("INSERT INTO Invoice(Id,Number,Amount) VALUES(@id,@n,@a)",
            new { id, n = req.Number, a = req.Amount });
        // §end

        // §4
        var data = new { Id = id, req.Number, req.Amount };
        // §end

        // §5
        var resp = Envelope.Ok(data, responseId);
        _audit.Update(responseId, 200, resp);
        return Ok(resp);
        // §end
    }
}
