// rendered from canon[Create] -> dotnet. 80% = phase order (accept/shred/guard/insert/project); 20% = Invoice slots
[ApiController][Route("invoice")]
public class InvoiceController : ControllerBase {
  [HttpPost] public IActionResult Create([FromBody] CreateInvoiceRequest req) {   // phase1 accept
    if (string.IsNullOrEmpty(req.Number)) return BadRequest(Envelope.Error("Number is required")); // phase3 guard (20%: required)
    var id = Guid.NewGuid();
    _db.Execute("INSERT INTO Invoice(Id,Number,Amount) VALUES(@id,@n,@a)", new{id, n=req.Number, a=req.Amount}); // phase4 INSERT (20%: dml_target+fields)
    return Ok(Envelope.Ok(new { Id=id, req.Number, req.Amount }));               // phase6 project (20%: response_shape)
  }
}
