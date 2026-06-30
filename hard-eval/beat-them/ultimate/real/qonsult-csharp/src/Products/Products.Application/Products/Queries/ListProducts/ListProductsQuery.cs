// Products.Application — ListProducts query. Backs GET /products/product/.

public record ListProductsQuery(bool FeaturedOnly = false);

public record ProductDto(Guid Id, string Sku, string Name, decimal MonthlyPrice, string CurrencyCode, bool IsFeatured);

public record ListProductsResponse(IReadOnlyList<ProductDto> Products);

public interface IListProductsService
{
    Task<Result<ListProductsResponse>> List(ListProductsQuery query, CancellationToken cancellationToken = default);
}

public class ListProductsService(IProductQueryRepository queryRepository) : IListProductsService
{
    public async Task<Result<ListProductsResponse>> List(ListProductsQuery query, CancellationToken cancellationToken = default)
    {
        // validate the request (none) / authorize (public catalog)
        // query via the read-side repository
        var products = query.FeaturedOnly
            ? await queryRepository.ListFeatured(cancellationToken)
            : await queryRepository.ListAll(cancellationToken);

        // map to DTO and return the response
        var dtos = products
            .Select(p => new ProductDto(p.Id, p.Sku, p.Name, p.MonthlyPrice, p.CurrencyCode, p.IsFeatured))
            .ToList();

        return Result.Success(new ListProductsResponse(dtos));
    }
}
