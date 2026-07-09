// Products.Infrastructure — implements both domain and query repositories for Product.

using Microsoft.EntityFrameworkCore;

public class ProductRepository(ProductsDbContext dbContext)
    : IProductDomainRepository, IProductQueryRepository
{
    public async Task Save(Product entity, CancellationToken cancellationToken = default)
    {
        var existing = await dbContext.Products
            .Include(p => p.Features)
            .FirstOrDefaultAsync(p => p.Id == entity.Id, cancellationToken);

        if (existing is null)
        {
            await dbContext.Products.AddAsync(entity, cancellationToken);
        }
        else
        {
            dbContext.Entry(existing).CurrentValues.SetValues(entity);
        }

        await dbContext.SaveChangesAsync(cancellationToken);
    }

    public async Task<Product?> Find(Guid id, CancellationToken cancellationToken = default)
        => await dbContext.Products
            .Include(p => p.Features)
            .FirstOrDefaultAsync(p => p.Id == id, cancellationToken);

    public async Task<Product?> FindBySku(string sku, CancellationToken cancellationToken = default)
    {
        var normalized = sku.Trim().ToUpperInvariant();
        return await dbContext.Products
            .Include(p => p.Features) // feature-data read needs the owned collection loaded
            .FirstOrDefaultAsync(p => p.Sku == normalized, cancellationToken);
    }

    public async Task<IReadOnlyList<Product>> ListAll(CancellationToken cancellationToken = default)
        => await dbContext.Products.AsNoTracking().OrderBy(p => p.Name).ToListAsync(cancellationToken);

    public async Task<IReadOnlyList<Product>> ListFeatured(CancellationToken cancellationToken = default)
        => await dbContext.Products.AsNoTracking()
            .Where(p => p.IsFeatured)
            .OrderBy(p => p.Name)
            .ToListAsync(cancellationToken);
}
