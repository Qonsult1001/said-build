// ============================================================================
//  Create Invoice (dotnet)   |   full section map + how to edit:  said.index.md
//  Each section is [Sn] ... [/Sn], in run order. The tag says if you may edit it:
//    GENERATED = .said wrote it and re-writes it -> do NOT edit (your changes are lost).
//                to change it: edit the canon, or run `said eject Sn` to take it over.
//    YOURS     = .said keeps your code here -> edit freely.
//
//    [S1] accept-and-audit   GENERATED
//    [S2] idempotency        GENERATED
//    [S3] guards             YOURS
//    [S4] save invoice       YOURS
//    [S5] response           YOURS
//    [S6] wrap + return      GENERATED
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
        // [S1] accept-and-audit  -- GENERATED (do not edit; `said eject S1` to take over)
        var responseId = Guid.NewGuid();
        _audit.Insert(req, responseId);
        // [/S1]

        // [S2] idempotency  -- GENERATED (do not edit)
        if (_seen.Contains(req.IdempotencyKey)) return Conflict();
        _seen.Add(req.IdempotencyKey);
        // [/S2]

        // [S3] guards  -- YOURS (edit freely; kept on regenerate)
        if (string.IsNullOrEmpty(req.Number))
            return BadRequest(Envelope.Error("Number is required"));
        // [/S3]

        // [S4] save invoice  -- YOURS (edit freely)
        var id = Guid.NewGuid();
        _db.Execute("INSERT INTO Invoice(Id,Number,Amount) VALUES(@id,@n,@a)",
            new { id, n = req.Number, a = req.Amount });
        // [/S4]

        // [S5] response  -- YOURS (edit freely)
        var data = new { Id = id, req.Number, req.Amount };
        // [/S5]

        // [S6] wrap + return  -- GENERATED (do not edit)
        var resp = Envelope.Ok(data, responseId);
        _audit.Update(responseId, 200, resp);
        return Ok(resp);
        // [/S6]
    }
}
