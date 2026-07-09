// Internal fluent builder for the Product aggregate. Build() asserts required fields then news it.
internal class ProductFactory : IProductFactory
{
    private string? name;
    private string productType = string.Empty;
    private decimal price;
    private string currency = string.Empty;
    private string? description;

    public IProductFactory WithName(string name)
    {
        this.name = name;
        return this;
    }

    public IProductFactory WithType(string productType)
    {
        this.productType = productType;
        return this;
    }

    public IProductFactory WithPricing(decimal price, string currency)
    {
        this.price = price;
        this.currency = currency;
        return this;
    }

    public IProductFactory WithDescription(string? description)
    {
        this.description = description;
        return this;
    }

    public Product Build()
    {
        if (string.IsNullOrWhiteSpace(name))
        {
            throw new InvalidOperationException("Cannot build a Product without a name.");
        }

        return new Product(name, productType, price, currency)
            .WithDescription(description);
    }
}
