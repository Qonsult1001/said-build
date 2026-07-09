using Microsoft.AspNetCore.Mvc;

// POST /mapche-api/v1/leads/ — records a lead and returns its reference_id. Inherits ApiController.
[Route("mapche-api/v1/leads")]
public class LeadsController(ICreateLeadService leads) : ApiController
{
    [HttpPost]
    public async Task<ActionResult> Create(CreateLeadCommand command)
        => await leads.Create(command).ToActionResult();
}
