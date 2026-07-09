// Search.Web — controller. Backs /mapche-api/v1/search/ and /mapche-api/v1/leads/.

using Microsoft.AspNetCore.Mvc;

public class MapcheController(
    ISearchCoverageService search,
    ICaptureLeadService captureLead) : ApiController
{
    [HttpGet]
    public async Task<ActionResult> Search([FromQuery] double latitude, [FromQuery] double longitude, [FromQuery] string? mapcheKey)
        => await search.Search(new SearchCoverageQuery(latitude, longitude, mapcheKey)).ToActionResult();

    [HttpPost]
    public async Task<ActionResult> Leads(CaptureLeadCommand command)
        => await captureLead.Capture(command).ToActionResult();
}
