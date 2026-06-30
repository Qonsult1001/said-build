// GET /mapche-api/v1/userprofile/?mapche_key=... — resolves the profile for a whitelabel client key.
public class GetUserProfileQuery
{
    public string MapcheKey { get; set; } = string.Empty;
}
