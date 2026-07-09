using System.Text.Json.Serialization;

// Read DTO for a preference row, carrying the business/reseller ids the frontend filters on.
public class UserPreferenceResponse
{
    public UserPreferenceResponse(Guid id, string mapcheKey, Guid businessId, Guid? resellerId, string providerName, bool isEnabled)
    {
        Id = id;
        MapcheKey = mapcheKey;
        BusinessId = businessId;
        ResellerId = resellerId;
        ProviderName = providerName;
        IsEnabled = isEnabled;
    }

    [JsonPropertyName("id")]
    public Guid Id { get; }

    [JsonPropertyName("mapche_key")]
    public string MapcheKey { get; }

    [JsonPropertyName("business_id")]
    public Guid BusinessId { get; }

    [JsonPropertyName("reseller_id")]
    public Guid? ResellerId { get; }

    [JsonPropertyName("provider_name")]
    public string ProviderName { get; }

    [JsonPropertyName("is_enabled")]
    public bool IsEnabled { get; }
}
