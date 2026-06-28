// @said-managed: index  canon=Create<Entity>  lang=dotnet  entity=Invoice  okf=said.index.md#invoice-create
//  S1 accept-and-audit   Fully    framework   (AI-authored-once, regenerated free)
//  S2 idempotency-key     AI       net-new     (AI hard-work, not in canon)
//  S3 guards             Ignore   entity-20%  (AI/human hard-work)
//  S4 dml                Ignore   entity-20%  (AI/human hard-work)
//  S5 response           Ignore   entity-20%  (AI/human hard-work)
//  S6 envelope+return    Fully    framework   (AI-authored-once, regenerated free)
// @said-managed: end
// NOTE: SN = EXECUTION ORDER (matches the body top-to-bottom), so the index is the file's flow map.
[ApiController]
[Route("invoice")]
public class InvoiceController : ControllerBase
{
    private readonly IDb _db;
    public InvoiceController(IDb db) { _db = db; }

    [HttpPost]
    public IActionResult Create([FromBody] CreateInvoiceRequest req)
    {
        // S1
        var responseId = Guid.NewGuid();
        _audit.Insert(req, responseId);
        // Send

        // S2 AI
        if (_seen.Contains(req.IdempotencyKey)) return Conflict();
        _seen.Add(req.IdempotencyKey);
        // Send

        // S3
        if (string.IsNullOrEmpty(req.Number))
            return BadRequest(Envelope.Error("Number is required"));
        // Send

        // S4
        var id = Guid.NewGuid();
        _db.Execute("INSERT INTO Invoice(Id,Number,Amount) VALUES(@id,@n,@a)",
            new { id, n = req.Number, a = req.Amount });
        // Send

        // S5
        var data = new { Id = id, req.Number, req.Amount };
        // Send

        // S6
        var resp = Envelope.Ok(data, responseId);
        _audit.Update(responseId, 200, resp);
        return Ok(resp);
        // Send
    }
}
