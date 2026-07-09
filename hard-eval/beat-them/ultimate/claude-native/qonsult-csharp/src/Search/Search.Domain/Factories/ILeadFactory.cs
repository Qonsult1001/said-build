// Fluent factory contract for the Lead aggregate.
public interface ILeadFactory
{
    ILeadFactory WithSearch(Guid searchId);

    ILeadFactory WithMapcheKey(string mapcheKey);

    ILeadFactory WithProduct(string product, decimal mrc, decimal nrc);

    ILeadFactory WithCommercials(string? productType, string? supplierName, string? description);

    ILeadFactory WithParties(Guid? businessId, Guid? shopperId);

    Lead Build();
}
