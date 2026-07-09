// Products.Domain — context-local events.

public class ProductCreatedEvent(Guid productId, string sku) : DomainEvent
{
    public Guid ProductId { get; } = productId;
    public string Sku { get; } = sku;
}
