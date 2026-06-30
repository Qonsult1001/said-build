using Microsoft.AspNetCore.Mvc;

// GET /mapche-api/v1/user-preferences/?mapche_key=... — returns the preference list. Inherits
// ApiController; the optional mapche_key binds from the query string.
[Route("mapche-api/v1/user-preferences")]
public class UserPreferencesController(IGetUserPreferencesService preferences) : ApiController
{
    [HttpGet]
    public async Task<ActionResult> Get([FromQuery(Name = "mapche_key")] string? mapcheKey)
        => await preferences.GetPreferences(new GetUserPreferencesQuery { MapcheKey = mapcheKey }).ToActionResult();
}
