using Microsoft.AspNetCore.Mvc;

// POST /accounts/token/login/ — authenticates and returns { auth_token }. Inherits ApiController;
// the explicit route matches the path the frontend calls. Action only delegates to the service.
[Route("accounts/token")]
public class AuthController(ILoginService login) : ApiController
{
    [HttpPost("login")]
    public async Task<ActionResult> Login(LoginCommand command)
        => await login.Login(command).ToActionResult();
}
