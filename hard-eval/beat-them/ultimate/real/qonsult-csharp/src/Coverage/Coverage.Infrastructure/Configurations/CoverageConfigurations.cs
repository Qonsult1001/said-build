// Coverage.Infrastructure — EF mappings for the GlobalCoverage aggregate.

using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Metadata.Builders;

public class GlobalCoverageConfiguration : IEntityTypeConfiguration<GlobalCoverage>
{
    public void Configure(EntityTypeBuilder<GlobalCoverage> builder)
    {
        builder.HasKey(c => c.Id);
        builder.Property(c => c.UserId).IsRequired();
        builder.HasIndex(c => c.UserId).IsUnique();

        builder.HasMany(c => c.Areas)
            .WithOne()
            .HasForeignKey(a => a.CoverageId)
            .OnDelete(DeleteBehavior.Cascade);

        builder.Navigation(c => c.Areas).UsePropertyAccessMode(PropertyAccessMode.Field);
        builder.Ignore(c => c.Events);
    }
}

public class CoverageAreaConfiguration : IEntityTypeConfiguration<CoverageArea>
{
    public void Configure(EntityTypeBuilder<CoverageArea> builder)
    {
        builder.HasKey(a => a.Id);
        builder.Property(a => a.SupplierName)
            .IsRequired()
            .HasMaxLength(CommonModelConstants.Common.MaxNameLength);
        builder.Property(a => a.Latitude);
        builder.Property(a => a.Longitude);
        builder.Property(a => a.RadiusKm);
        builder.Ignore(a => a.Events);
    }
}
