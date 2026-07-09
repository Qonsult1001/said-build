using System.Text.Json.Serialization;

// POST /mapche-api/v1/search/ — the address-component payload the frontend posts. JSON names match the
// keys it sends (mapche_key, location_lat, address_component_*).
public class CreateSearchCommand
{
    [JsonPropertyName("mapche_key")]
    public string MapcheKey { get; set; } = string.Empty;

    [JsonPropertyName("formatted_address")]
    public string FormattedAddress { get; set; } = string.Empty;

    [JsonPropertyName("location_lat")]
    public double LocationLat { get; set; }

    [JsonPropertyName("location_lng")]
    public double LocationLng { get; set; }

    [JsonPropertyName("place_id")]
    public string? PlaceId { get; set; }

    [JsonPropertyName("address_component_country")]
    public string CountryName { get; set; } = string.Empty;

    [JsonPropertyName("address_component_country_short")]
    public string CountryShort { get; set; } = string.Empty;

    [JsonPropertyName("address_component_province")]
    public string? Province { get; set; }

    [JsonPropertyName("address_component_town")]
    public string? Town { get; set; }

    [JsonPropertyName("address_component_suburb")]
    public string? Suburb { get; set; }

    [JsonPropertyName("address_component_postal_code")]
    public string? PostalCode { get; set; }

    [JsonPropertyName("infoip")]
    public string? InfoIp { get; set; }

    [JsonPropertyName("infocity")]
    public string? InfoCity { get; set; }

    [JsonPropertyName("infocountry")]
    public string? InfoCountry { get; set; }

    [JsonPropertyName("mobile")]
    public string? Mobile { get; set; }
}
