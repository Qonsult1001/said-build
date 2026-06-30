using Microsoft.AspNetCore.Mvc;

// GET /mapche-api/v1/userprofile/?mapche_key=... — returns the whitelabel profile. Inherits
// ApiController; query params bind onto the query object. No business or data logic here.
[Route("mapche-api/v1/userprofile")]
public class UserProfileController(IGetUserProfileService profiles) : ApiController
{
    [HttpGet]
    public async Task<ActionResult> Get([FromQuery(Name = "mapche_key")] string mapcheKey)
        => await profiles.GetProfile(new GetUserProfileQuery { MapcheKey = mapcheKey }).ToActionResult();
}
