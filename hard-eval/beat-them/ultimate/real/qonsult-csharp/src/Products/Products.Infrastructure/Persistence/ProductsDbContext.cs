// Products.Infrastructure — EF Core DbContext for the Products context.

using Microsoft.EntityFrameworkCore;

public class ProductsDbContext(DbContextOptions<ProductsDbContext> options) : DbContext(options)
{
    public DbSet<Product> Products => Set<Product>();
    public DbSet<ProductFeature> ProductFeatures => Set<ProductFeature>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.ApplyConfiguration(new ProductConfiguration());
        modelBuilder.ApplyConfiguration(new ProductFeatureConfiguration());
        base.OnModelCreating(modelBuilder);
    }
}
