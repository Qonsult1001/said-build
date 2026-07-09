// Create Invoice (dotnet).  Sections: [Sn]..[/Sn] in run order.  Map + how to edit: said.index.md
// Tag on each section = can you edit it?   GENERATED (no, .said rewrites it) | YOURS (yes, kept).
[ApiController]
[Route("invoice")]
public class InvoiceController : ControllerBase
{
    private readonly IDb _db;
    public InvoiceController(IDb db) { _db = db; }

    [HttpPost]
    public IActionResult Create([FromBody] CreateInvoiceRequest req)
    {
        // [S1] accept-and-audit  GENERATED
        var responseId = Guid.NewGuid();
        _audit.Insert(req, responseId);
        // [/S1]

        // [S2] idempotency  GENERATED
        if (_seen.Contains(req.IdempotencyKey)) return Conflict();
        _seen.Add(req.IdempotencyKey);
        // [/S2]

        // [S3] guards  YOURS
        if (string.IsNullOrEmpty(req.Number))
            return BadRequest(Envelope.Error("Number is required"));
        // [/S3]

        // [S4] save invoice  YOURS
        var id = Guid.NewGuid();
        _db.Execute("INSERT INTO Invoice(Id,Number,Amount) VALUES(@id,@n,@a)",
            new { id, n = req.Number, a = req.Amount });
        // [/S4]

        // [S5] response  YOURS
        var data = new { Id = id, req.Number, req.Amount };
        // [/S5]

        // [S6] wrap + return  GENERATED
        var resp = Envelope.Ok(data, responseId);
        _audit.Update(responseId, 200, resp);
        return Ok(resp);
        // [/S6]
    }
}
