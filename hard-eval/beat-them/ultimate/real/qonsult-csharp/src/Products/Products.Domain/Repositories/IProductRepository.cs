// Products.Domain — repository interfaces only.

public interface IProductDomainRepository : IDomainRepository<Product>
{
    Task<Product?> FindBySku(string sku, CancellationToken cancellationToken = default);
}

public interface IProductQueryRepository : IQueryRepository
{
    Task<IReadOnlyList<Product>> ListAll(CancellationToken cancellationToken = default);
    Task<IReadOnlyList<Product>> ListFeatured(CancellationToken cancellationToken = default);
}
