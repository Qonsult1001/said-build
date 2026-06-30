// Products.Domain — fluent factory for Product.

public interface IProductFactory
{
    IProductFactory WithSku(string sku);
    IProductFactory WithName(string name);
    IProductFactory WithPrice(decimal monthlyPrice, string currencyCode);
    Product Build();
}

internal class ProductFactory : IProductFactory
{
    private string? _sku;
    private string? _name;
    private decimal? _monthlyPrice;
    private string? _currencyCode;

    public IProductFactory WithSku(string sku)
    {
        _sku = sku;
        return this;
    }

    public IProductFactory WithName(string name)
    {
        _name = name;
        return this;
    }

    public IProductFactory WithPrice(decimal monthlyPrice, string currencyCode)
    {
        _monthlyPrice = monthlyPrice;
        _currencyCode = currencyCode;
        return this;
    }

    public Product Build()
    {
        if (_sku is null || _name is null || _monthlyPrice is null || _currencyCode is null)
        {
            throw new InvalidOperationException("Sku, name and price are required to build a Product.");
        }

        return new Product(_sku, _name, _monthlyPrice.Value, _currencyCode);
    }
}
