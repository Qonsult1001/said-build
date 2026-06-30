using Microsoft.AspNetCore.Mvc;

// GET /coverage/userglobalcoverages/ — returns the coverage list. Inherits ApiController; action only
// delegates to the service.
[Route("coverage")]
public class CoverageController(IGetUserGlobalCoveragesService coverages) : ApiController
{
    [HttpGet("userglobalcoverages")]
    public async Task<ActionResult> GetUserGlobalCoverages()
        => await coverages.GetCoverages(new GetUserGlobalCoveragesQuery()).ToActionResult();
}
