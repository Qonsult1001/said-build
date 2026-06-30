// Products.Infrastructure — EF mappings for the Product aggregate.

using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Metadata.Builders;

public class ProductConfiguration : IEntityTypeConfiguration<Product>
{
    public void Configure(EntityTypeBuilder<Product> builder)
    {
        builder.HasKey(p => p.Id);
        builder.Property(p => p.Sku).IsRequired().HasMaxLength(64);
        builder.HasIndex(p => p.Sku).IsUnique();
        builder.Property(p => p.Name).IsRequired().HasMaxLength(CommonModelConstants.Common.MaxNameLength);
        builder.Property(p => p.MonthlyPrice).HasPrecision(18, 2);
        builder.Property(p => p.CurrencyCode).IsRequired().HasMaxLength(3);
        builder.Property(p => p.IsFeatured);

        builder.HasMany(p => p.Features)
            .WithOne()
            .HasForeignKey(f => f.ProductId)
            .OnDelete(DeleteBehavior.Cascade);

        builder.Navigation(p => p.Features).UsePropertyAccessMode(PropertyAccessMode.Field);
        builder.Ignore(p => p.Events);
    }
}

public class ProductFeatureConfiguration : IEntityTypeConfiguration<ProductFeature>
{
    public void Configure(EntityTypeBuilder<ProductFeature> builder)
    {
        builder.HasKey(f => f.Id);
        builder.Property(f => f.Label).IsRequired().HasMaxLength(CommonModelConstants.Common.MaxNameLength);
        builder.Property(f => f.Value).HasMaxLength(1024);
        builder.Ignore(f => f.Events);
    }
}
