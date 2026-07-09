// Products.Domain — the Product aggregate. Backs /products/product/.

public class Product : Entity, IAggregateRoot
{
    private readonly List<ProductFeature> _features = new();

    internal Product(string sku, string name, decimal monthlyPrice, string currencyCode)
    {
        ValidateSku(sku);
        ValidateName(name);
        ValidatePrice(monthlyPrice);
        ValidateCurrency(currencyCode);

        Sku = sku.Trim().ToUpperInvariant();
        Name = name.Trim();
        MonthlyPrice = monthlyPrice;
        CurrencyCode = currencyCode.Trim().ToUpperInvariant();
        IsFeatured = false;

        RaiseEvent(new ProductCreatedEvent(Id, Sku));
    }

    private Product() { } // EF

    public string Sku { get; private set; } = default!;
    public string Name { get; private set; } = default!;
    public decimal MonthlyPrice { get; private set; }
    public string CurrencyCode { get; private set; } = default!;
    public bool IsFeatured { get; private set; }
    public IReadOnlyCollection<ProductFeature> Features => _features.AsReadOnly();

    public Product AddFeature(string label, string value)
    {
        if (string.IsNullOrWhiteSpace(label))
        {
            throw new ArgumentException("Feature label is required.", nameof(label));
        }

        _features.Add(new ProductFeature(Id, label.Trim(), value?.Trim() ?? string.Empty));
        return this;
    }

    public Product Feature()
    {
        IsFeatured = true;
        return this;
    }

    private static void ValidateSku(string sku)
    {
        if (string.IsNullOrWhiteSpace(sku))
        {
            throw new ArgumentException("SKU is required.", nameof(sku));
        }
    }

    private static void ValidateName(string name)
    {
        if (string.IsNullOrWhiteSpace(name))
        {
            throw new ArgumentException("Product name is required.", nameof(name));
        }
    }

    private static void ValidatePrice(decimal price)
    {
        if (price < 0)
        {
            throw new ArgumentException("Price cannot be negative.", nameof(price));
        }
    }

    private static void ValidateCurrency(string currencyCode)
    {
        if (string.IsNullOrWhiteSpace(currencyCode) || currencyCode.Trim().Length != 3)
        {
            throw new ArgumentException("CurrencyCode must be a 3-letter ISO code.", nameof(currencyCode));
        }
    }
}
