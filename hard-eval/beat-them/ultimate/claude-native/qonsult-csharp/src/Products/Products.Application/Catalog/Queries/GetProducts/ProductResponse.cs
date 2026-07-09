using System.Text.Json.Serialization;

// Read DTO for a catalog product.
public class ProductResponse
{
    public ProductResponse(Guid id, string name, string productType, decimal price, string currency, string? description)
    {
        Id = id;
        Name = name;
        ProductType = productType;
        Price = price;
        Currency = currency;
        Description = description;
    }

    [JsonPropertyName("id")]
    public Guid Id { get; }

    [JsonPropertyName("name")]
    public string Name { get; }

    [JsonPropertyName("producttype")]
    public string ProductType { get; }

    [JsonPropertyName("price")]
    public decimal Price { get; }

    [JsonPropertyName("currency")]
    public string Currency { get; }

    [JsonPropertyName("description")]
    public string? Description { get; }
}
