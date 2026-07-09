using System.Text.Json.Serialization;

// Read DTO for a single coverage row. JSON names match the fields the frontend reads
// (country_name, currency, business_id).
public class GlobalCoverageResponse
{
    public GlobalCoverageResponse(Guid id, Guid businessId, string countryName, string countryCode, string currency)
    {
        Id = id;
        BusinessId = businessId;
        CountryName = countryName;
        CountryCode = countryCode;
        Currency = currency;
    }

    [JsonPropertyName("id")]
    public Guid Id { get; }

    [JsonPropertyName("business_id")]
    public Guid BusinessId { get; }

    [JsonPropertyName("country_name")]
    public string CountryName { get; }

    [JsonPropertyName("country_short")]
    public string CountryCode { get; }

    [JsonPropertyName("currency")]
    public string Currency { get; }
}
