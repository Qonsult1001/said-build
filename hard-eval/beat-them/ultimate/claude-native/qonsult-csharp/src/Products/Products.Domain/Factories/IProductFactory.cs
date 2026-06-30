// Fluent factory contract for the Product aggregate.
public interface IProductFactory
{
    IProductFactory WithName(string name);

    IProductFactory WithType(string productType);

    IProductFactory WithPricing(decimal price, string currency);

    IProductFactory WithDescription(string? description);

    Product Build();
}
