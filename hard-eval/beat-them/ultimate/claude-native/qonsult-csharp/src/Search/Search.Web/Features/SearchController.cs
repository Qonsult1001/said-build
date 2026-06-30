using Microsoft.AspNetCore.Mvc;

// POST /mapche-api/v1/search/ — records a search and returns its id + resolved country. Inherits
// ApiController; action only delegates to the service.
[Route("mapche-api/v1/search")]
public class SearchController(ICreateSearchService searches) : ApiController
{
    [HttpPost]
    public async Task<ActionResult> Create(CreateSearchCommand command)
        => await searches.Create(command).ToActionResult();
}
