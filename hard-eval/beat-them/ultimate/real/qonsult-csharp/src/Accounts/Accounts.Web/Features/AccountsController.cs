// Accounts.Web — controller. One-liner actions delegating to Application services.
// Backs /accounts/token/login and account registration.

using Microsoft.AspNetCore.Mvc;

public class AccountsController(
    IRegisterAccountService registerAccount,
    ILoginService login) : ApiController
{
    [HttpPost]
    public async Task<ActionResult> Register(RegisterAccountCommand command)
        => await registerAccount.Register(command).ToActionResult();

    // POST /api/accounts/login  (the token/login endpoint the frontend calls)
    [HttpPost]
    public async Task<ActionResult> Login(LoginCommand command)
        => await login.Login(command).ToActionResult();
}
