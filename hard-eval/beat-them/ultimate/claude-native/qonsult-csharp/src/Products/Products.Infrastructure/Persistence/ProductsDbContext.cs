using Microsoft.EntityFrameworkCore;

// EF Core context owning the Products schema (feature data + catalog).
public class ProductsDbContext(DbContextOptions<ProductsDbContext> options) : DbContext(options)
{
    public DbSet<FeatureDatum> FeatureData => Set<FeatureDatum>();

    public DbSet<Product> Products => Set<Product>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<FeatureDatum>(feature =>
        {
            feature.HasKey(f => f.Id);
            feature.Property(f => f.Name).IsRequired().HasMaxLength(ProductModelConstants.FeatureNameMaxLength);
            feature.HasIndex(f => f.Name).IsUnique();
            feature.Ignore(f => f.Events);
        });

        modelBuilder.Entity<Product>(product =>
        {
            product.HasKey(p => p.Id);
            product.Property(p => p.Name).IsRequired().HasMaxLength(ProductModelConstants.ProductNameMaxLength);
            product.Property(p => p.Price).HasColumnType("decimal(18,2)");
            product.Ignore(p => p.Events);
        });

        base.OnModelCreating(modelBuilder);
    }
}
