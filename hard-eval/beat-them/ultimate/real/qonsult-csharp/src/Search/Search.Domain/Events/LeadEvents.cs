// Search.Domain — context-local events.

public class LeadCapturedEvent(Guid leadId, string mapcheKey, string contactNumber) : DomainEvent
{
    public Guid LeadId { get; } = leadId;
    public string MapcheKey { get; } = mapcheKey;
    public string ContactNumber { get; } = contactNumber;
}

public class LeadConvertedEvent(Guid leadId, string mapcheKey) : DomainEvent
{
    public Guid LeadId { get; } = leadId;
    public string MapcheKey { get; } = mapcheKey;
}
