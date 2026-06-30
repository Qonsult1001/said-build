// Search context — Lead aggregate root. A shopper's request for a service against a prior Search.
// POST /mapche-api/v1/leads/ creates one and returns a generated reference_id. Private setters.
public class Lead : Entity, IAggregateRoot
{
    private Lead()
    {
    }

    public Lead(Guid searchId, string mapcheKey, string product, decimal mrc, decimal nrc)
    {
        ValidateSearchId(searchId);
        ValidateMapcheKey(mapcheKey);
        ValidateProduct(product);

        SearchId = searchId;
        MapcheKey = mapcheKey;
        Product = product;
        Mrc = mrc;
        Nrc = nrc;
        ReferenceId = GenerateReferenceId();
        CreatedAt = DateTime.UtcNow;
    }

    public Guid SearchId { get; private set; }

    public string MapcheKey { get; private set; } = string.Empty;

    public Guid? BusinessId { get; private set; }

    public Guid? ShopperId { get; private set; }

    public string Product { get; private set; } = string.Empty;

    public string? ProductType { get; private set; }

    public string? SupplierName { get; private set; }

    public decimal Mrc { get; private set; }

    public decimal Nrc { get; private set; }

    public string? Description { get; private set; }

    public string ReferenceId { get; private set; } = string.Empty;

    public DateTime CreatedAt { get; private set; }

    // Attaches the commercial detail the frontend submits with the lead.
    public Lead WithCommercials(string? productType, string? supplierName, string? description)
    {
        ProductType = productType;
        SupplierName = supplierName;
        Description = description;
        return this;
    }

    // Associates the lead with the originating business and shopper.
    public Lead WithParties(Guid? businessId, Guid? shopperId)
    {
        BusinessId = businessId;
        ShopperId = shopperId;
        return this;
    }

    private static string GenerateReferenceId()
        => $"LEAD-{DateTime.UtcNow:yyyyMMdd}-{Guid.NewGuid().ToString("N")[..8].ToUpperInvariant()}";

    private static void ValidateSearchId(Guid searchId)
    {
        if (searchId == Guid.Empty)
        {
            throw new InvalidOperationException("A lead must reference a search.");
        }
    }

    private static void ValidateMapcheKey(string mapcheKey)
    {
        if (string.IsNullOrWhiteSpace(mapcheKey))
        {
            throw new InvalidOperationException("mapche_key is required to record a lead.");
        }
    }

    private static void ValidateProduct(string product)
    {
        if (string.IsNullOrWhiteSpace(product))
        {
            throw new InvalidOperationException("A product is required on a lead.");
        }
    }
}
