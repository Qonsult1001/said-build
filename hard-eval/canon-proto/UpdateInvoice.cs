// Update Invoice (dotnet).  Sections: [Sn]..[/Sn] in run order.  Map + how to edit: said.index.md
// Tag = can you edit it?  GENERATED (no, .said rewrites it) | YOURS (yes, kept).
    public IActionResult Update(Guid id, [FromBody] UpdateInvoiceRequest req)
    {
        // [S1] accept-and-audit  GENERATED
        var responseId = Guid.NewGuid();
        // [/S1]
        // [S2] guards  YOURS
        if (req.Amount < 0) return BadRequest("Amount must be >= 0");
        // [/S2]
        // [S3] update row  YOURS
        _db.Execute("UPDATE Invoice SET Amount=@a WHERE Id=@id", new { id, a = req.Amount });
        // [/S3]
        // [S4] wrap + return  GENERATED
        return Ok(Envelope.Ok(new { id }, responseId));
        // [/S4]
    }
