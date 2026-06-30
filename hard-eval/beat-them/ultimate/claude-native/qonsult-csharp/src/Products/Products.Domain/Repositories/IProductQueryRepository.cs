// Read-side repository for the product catalog. Interface in Domain; implementation in
// Products.Infrastructure.
public interface IProductQueryRepository
{
    Task<IReadOnlyList<Product>> GetAll(CancellationToken cancellationToken = default);
}
