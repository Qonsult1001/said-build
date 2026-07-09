// POST /accounts/token/login/ — credential payload sent by the frontend (username + password form).
public class LoginCommand
{
    public string Username { get; set; } = string.Empty;

    public string Password { get; set; } = string.Empty;
}
