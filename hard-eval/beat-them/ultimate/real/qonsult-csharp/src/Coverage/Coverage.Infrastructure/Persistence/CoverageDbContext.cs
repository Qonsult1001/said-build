// Coverage.Infrastructure — EF Core DbContext for the Coverage context.

using Microsoft.EntityFrameworkCore;

public class CoverageDbContext(DbContextOptions<CoverageDbContext> options) : DbContext(options)
{
    public DbSet<GlobalCoverage> Coverages => Set<GlobalCoverage>();
    public DbSet<CoverageArea> CoverageAreas => Set<CoverageArea>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.ApplyConfiguration(new GlobalCoverageConfiguration());
        modelBuilder.ApplyConfiguration(new CoverageAreaConfiguration());
        base.OnModelCreating(modelBuilder);
    }
}
