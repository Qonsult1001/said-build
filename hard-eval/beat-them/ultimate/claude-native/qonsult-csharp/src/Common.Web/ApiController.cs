using Microsoft.AspNetCore.Mvc;

// Base controller every feature controller inherits. Gives the conventional route
// api/[controller]/[action]. Controllers hold no business or data logic — every action delegates to
// an Application service and ends in .ToActionResult().
[ApiController]
[Route("api/[controller]/[action]")]
public abstract class ApiController : ControllerBase
{
}
