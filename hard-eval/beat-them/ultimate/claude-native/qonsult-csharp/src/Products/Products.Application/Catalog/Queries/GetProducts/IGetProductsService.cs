// Use-case contract for listing the product catalog.
public interface IGetProductsService
{
    Task<Result<IReadOnlyList<ProductResponse>>> GetProducts(
        GetProductsQuery query,
        CancellationToken cancellationToken = default);
}
