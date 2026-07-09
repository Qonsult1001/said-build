using System.Text.Json.Serialization;

// Read DTO for a feature row. `name` is the field the frontend matches on.
public class FeatureDataResponse
{
    public FeatureDataResponse(Guid id, string name, string? description, bool isActive)
    {
        Id = id;
        Name = name;
        Description = description;
        IsActive = isActive;
    }

    [JsonPropertyName("id")]
    public Guid Id { get; }

    [JsonPropertyName("name")]
    public string Name { get; }

    [JsonPropertyName("description")]
    public string? Description { get; }

    [JsonPropertyName("is_active")]
    public bool IsActive { get; }
}
