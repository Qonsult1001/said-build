using Microsoft.EntityFrameworkCore;

// EF Core context owning the Coverage schema.
public class CoverageDbContext(DbContextOptions<CoverageDbContext> options) : DbContext(options)
{
    public DbSet<GlobalCoverage> GlobalCoverages => Set<GlobalCoverage>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<GlobalCoverage>(coverage =>
        {
            coverage.HasKey(c => c.Id);
            coverage.Property(c => c.CountryName).IsRequired().HasMaxLength(CoverageModelConstants.CountryNameMaxLength);
            coverage.Property(c => c.Currency).IsRequired().HasMaxLength(CoverageModelConstants.CurrencyCodeMaxLength);
            coverage.HasIndex(c => c.BusinessId);
            coverage.Ignore(c => c.Events);
        });

        base.OnModelCreating(modelBuilder);
    }
}
