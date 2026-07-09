using System.Text.Json.Serialization;

// POST /mapche-api/v1/leads/ — the lead payload the frontend posts (business, shopper, search +
// commercial fields).
public class CreateLeadCommand
{
    [JsonPropertyName("mapche_key")]
    public string MapcheKey { get; set; } = string.Empty;

    [JsonPropertyName("search")]
    public Guid Search { get; set; }

    [JsonPropertyName("business")]
    public Guid? Business { get; set; }

    [JsonPropertyName("shopper")]
    public Guid? Shopper { get; set; }

    [JsonPropertyName("product")]
    public string Product { get; set; } = string.Empty;

    [JsonPropertyName("producttype")]
    public string? ProductType { get; set; }

    [JsonPropertyName("suppliername")]
    public string? SupplierName { get; set; }

    [JsonPropertyName("mrc")]
    public decimal Mrc { get; set; }

    [JsonPropertyName("nrc")]
    public decimal Nrc { get; set; }

    [JsonPropertyName("description")]
    public string? Description { get; set; }
}
