// Products context — Product aggregate root. A sellable product the frontend lists via
// GET /products/product/. Private setters; invariants in-aggregate.
public class Product : Entity, IAggregateRoot
{
    private Product()
    {
    }

    public Product(string name, string productType, decimal price, string currency)
    {
        ValidateName(name);
        ValidatePrice(price);

        Name = name;
        ProductType = productType ?? string.Empty;
        Price = price;
        Currency = currency ?? string.Empty;
        IsActive = true;
    }

    public string Name { get; private set; } = string.Empty;

    public string ProductType { get; private set; } = string.Empty;

    public decimal Price { get; private set; }

    public string Currency { get; private set; } = string.Empty;

    public string? Description { get; private set; }

    public bool IsActive { get; private set; }

    public Product WithDescription(string? description)
    {
        Description = description;
        return this;
    }

    private static void ValidateName(string name)
    {
        if (string.IsNullOrWhiteSpace(name))
        {
            throw new InvalidOperationException("A product name is required.");
        }

        if (name.Length > ProductModelConstants.ProductNameMaxLength)
        {
            throw new InvalidOperationException(
                $"Product name must be at most {ProductModelConstants.ProductNameMaxLength} characters.");
        }
    }

    private static void ValidatePrice(decimal price)
    {
        if (price < 0)
        {
            throw new InvalidOperationException("Price must not be negative.");
        }
    }
}
