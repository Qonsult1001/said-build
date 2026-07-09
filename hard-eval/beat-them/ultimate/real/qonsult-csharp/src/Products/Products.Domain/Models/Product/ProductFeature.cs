// Products.Domain — ProductFeature entity, part of the Product aggregate. Backs /products/featuredata/.

public class ProductFeature : Entity
{
    internal ProductFeature(Guid productId, string label, string value)
    {
        ProductId = productId;
        Label = label;
        Value = value;
    }

    private ProductFeature() { } // EF

    public Guid ProductId { get; private set; }
    public string Label { get; private set; } = default!;
    public string Value { get; private set; } = default!;
}
