// Internal fluent builder for the Lead aggregate. Build() asserts required fields then news it.
internal class LeadFactory : ILeadFactory
{
    private Guid searchId;
    private string? mapcheKey;
    private string? product;
    private decimal mrc;
    private decimal nrc;
    private string? productType;
    private string? supplierName;
    private string? description;
    private Guid? businessId;
    private Guid? shopperId;

    public ILeadFactory WithSearch(Guid searchId)
    {
        this.searchId = searchId;
        return this;
    }

    public ILeadFactory WithMapcheKey(string mapcheKey)
    {
        this.mapcheKey = mapcheKey;
        return this;
    }

    public ILeadFactory WithProduct(string product, decimal mrc, decimal nrc)
    {
        this.product = product;
        this.mrc = mrc;
        this.nrc = nrc;
        return this;
    }

    public ILeadFactory WithCommercials(string? productType, string? supplierName, string? description)
    {
        this.productType = productType;
        this.supplierName = supplierName;
        this.description = description;
        return this;
    }

    public ILeadFactory WithParties(Guid? businessId, Guid? shopperId)
    {
        this.businessId = businessId;
        this.shopperId = shopperId;
        return this;
    }

    public Lead Build()
    {
        if (searchId == Guid.Empty)
        {
            throw new InvalidOperationException("Cannot build a Lead without a search reference.");
        }

        if (string.IsNullOrWhiteSpace(mapcheKey))
        {
            throw new InvalidOperationException("Cannot build a Lead without a mapche_key.");
        }

        if (string.IsNullOrWhiteSpace(product))
        {
            throw new InvalidOperationException("Cannot build a Lead without a product.");
        }

        return new Lead(searchId, mapcheKey, product, mrc, nrc)
            .WithCommercials(productType, supplierName, description)
            .WithParties(businessId, shopperId);
    }
}
