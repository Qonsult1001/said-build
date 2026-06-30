using Microsoft.EntityFrameworkCore;

// Read-side repository implementation for the product catalog.
public class ProductRepository(ProductsDbContext context) : IProductQueryRepository
{
    public async Task<IReadOnlyList<Product>> GetAll(CancellationToken cancellationToken = default)
        => await context.Products
            .AsNoTracking()
            .Where(p => p.IsActive)
            .ToListAsync(cancellationToken);
}
