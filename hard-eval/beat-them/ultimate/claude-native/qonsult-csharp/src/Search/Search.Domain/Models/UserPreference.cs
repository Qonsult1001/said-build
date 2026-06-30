// Search context — UserPreference aggregate root. Whitelabel/provider preferences keyed by mapche_key,
// read by GET /mapche-api/v1/user-preferences/ and filtered client-side by business/reseller.
public class UserPreference : Entity, IAggregateRoot
{
    private UserPreference()
    {
    }

    public UserPreference(string mapcheKey, Guid businessId, string providerName)
    {
        if (string.IsNullOrWhiteSpace(mapcheKey))
        {
            throw new InvalidOperationException("mapche_key is required on a preference.");
        }

        MapcheKey = mapcheKey;
        BusinessId = businessId;
        ProviderName = providerName ?? string.Empty;
    }

    public string MapcheKey { get; private set; } = string.Empty;

    public Guid BusinessId { get; private set; }

    public Guid? ResellerId { get; private set; }

    public string ProviderName { get; private set; } = string.Empty;

    public bool IsEnabled { get; private set; } = true;

    public UserPreference ForReseller(Guid resellerId)
    {
        ResellerId = resellerId;
        return this;
    }
}
