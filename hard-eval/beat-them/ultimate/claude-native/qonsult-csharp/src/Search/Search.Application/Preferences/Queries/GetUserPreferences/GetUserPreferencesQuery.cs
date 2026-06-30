// GET /mapche-api/v1/user-preferences/?mapche_key=... — optional mapche_key filter; when absent the
// frontend filters the full list client-side by business/reseller.
public class GetUserPreferencesQuery
{
    public string? MapcheKey { get; set; }
}
