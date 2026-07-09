// List the product catalog. Same query shape as GetFeatureData: read via the query repository -> map
// each aggregate to a read DTO -> return Result<T>.
public class GetProductsService(
    IProductQueryRepository products) : IGetProductsService
{
    public async Task<Result<IReadOnlyList<ProductResponse>>> GetProducts(
        GetProductsQuery query,
        CancellationToken cancellationToken = default)
    {
        var all = await products.GetAll(cancellationToken);

        IReadOnlyList<ProductResponse> response = all
            .Select(p => new ProductResponse(p.Id, p.Name, p.ProductType, p.Price, p.Currency, p.Description))
            .ToList();

        return Result<IReadOnlyList<ProductResponse>>.SuccessWith(response);
    }
}
