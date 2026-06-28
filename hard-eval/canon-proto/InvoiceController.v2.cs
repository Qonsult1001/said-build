// ============================================================================
//  CANON: Create<Entity>   entity=Invoice   lang=dotnet
//  Section map + audit:  said.index.md          <- Ctrl/Cmd-click this path (IDE auto-links bare paths)
//  Sections run top-to-bottom; each is [Sn] ... [/Sn]. Class shown on the open tag.
//    [S1] accept-and-audit   Fully    framework  (regenerated free)
//    [S2] idempotency        AI       net-new    (AI wrote this)
//    [S3] guards             Ignore   entity-20%
//    [S4] dml                Ignore   entity-20%
//    [S5] response           Ignore   entity-20%
//    [S6] envelope+return    Fully    framework  (regenerated free)
// ============================================================================
[ApiController]
[Route("invoice")]
public class InvoiceController : ControllerBase
{
    private readonly IDb _db;
    public InvoiceController(IDb db) { _db = db; }

    [HttpPost]
    public IActionResult Create([FromBody] CreateInvoiceRequest req)
    {
        // [S1] accept-and-audit  (Fully: framework)
        var responseId = Guid.NewGuid();
        _audit.Insert(req, responseId);
        // [/S1]

        // [S2] idempotency  (AI: net-new)
        if (_seen.Contains(req.IdempotencyKey)) return Conflict();
        _seen.Add(req.IdempotencyKey);
        // [/S2]

        // [S3] guards  (Ignore: entity-20%)
        if (string.IsNullOrEmpty(req.Number))
            return BadRequest(Envelope.Error("Number is required"));
        // [/S3]

        // [S4] dml  (Ignore: entity-20%)
        var id = Guid.NewGuid();
        _db.Execute("INSERT INTO Invoice(Id,Number,Amount) VALUES(@id,@n,@a)",
            new { id, n = req.Number, a = req.Amount });
        // [/S4]

        // [S5] response  (Ignore: entity-20%)
        var data = new { Id = id, req.Number, req.Amount };
        // [/S5]

        // [S6] envelope+return  (Fully: framework)
        var resp = Envelope.Ok(data, responseId);
        _audit.Update(responseId, 200, resp);
        return Ok(resp);
        // [/S6]
    }
}
