using System.Text.Json.Serialization;

// Response DTO for login. The frontend reads response.data.auth_token, so the JSON name must be
// auth_token. Never returns the User aggregate.
public class LoginResponse
{
    public LoginResponse(string authToken)
    {
        AuthToken = authToken;
    }

    [JsonPropertyName("auth_token")]
    public string AuthToken { get; }
}
