using System.Text.Json.Serialization;

// Read DTO for the user profile the frontend renders (whitelabel preferences keyed by mapche_key).
public class UserProfileResponse
{
    public UserProfileResponse(Guid id, string username, string email, string? firstName, string? lastName, string? mapcheKey)
    {
        Id = id;
        Username = username;
        Email = email;
        FirstName = firstName;
        LastName = lastName;
        MapcheKey = mapcheKey;
    }

    [JsonPropertyName("id")]
    public Guid Id { get; }

    [JsonPropertyName("username")]
    public string Username { get; }

    [JsonPropertyName("email")]
    public string Email { get; }

    [JsonPropertyName("first_name")]
    public string? FirstName { get; }

    [JsonPropertyName("last_name")]
    public string? LastName { get; }

    [JsonPropertyName("mapche_key")]
    public string? MapcheKey { get; }
}
