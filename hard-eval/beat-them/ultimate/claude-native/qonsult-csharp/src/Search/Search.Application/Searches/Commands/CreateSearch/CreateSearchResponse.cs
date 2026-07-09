using System.Text.Json.Serialization;

// Response DTO for a created search. The frontend reads response.data.id and the resolved country
// (address_component_country / _short), so those JSON names are reproduced exactly.
public class CreateSearchResponse
{
    public CreateSearchResponse(Guid id, string countryName, string countryShort)
    {
        Id = id;
        CountryName = countryName;
        CountryShort = countryShort;
    }

    [JsonPropertyName("id")]
    public Guid Id { get; }

    [JsonPropertyName("address_component_country")]
    public string CountryName { get; }

    [JsonPropertyName("address_component_country_short")]
    public string CountryShort { get; }
}
