using System.Text.Json.Serialization;

// Response DTO for a created lead. The frontend reads response.data.reference_id.
public class CreateLeadResponse
{
    public CreateLeadResponse(Guid id, string referenceId)
    {
        Id = id;
        ReferenceId = referenceId;
    }

    [JsonPropertyName("id")]
    public Guid Id { get; }

    [JsonPropertyName("reference_id")]
    public string ReferenceId { get; }
}
