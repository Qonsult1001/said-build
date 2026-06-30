// Coverage.Domain — context-local events.

public class CoverageAreaAddedEvent(Guid coverageId, Guid userId, string supplierName) : DomainEvent
{
    public Guid CoverageId { get; } = coverageId;
    public Guid UserId { get; } = userId;
    public string SupplierName { get; } = supplierName;
}
