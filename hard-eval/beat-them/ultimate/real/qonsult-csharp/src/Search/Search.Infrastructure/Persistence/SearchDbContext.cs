// Search.Infrastructure — EF Core DbContext for the Search context.

using Microsoft.EntityFrameworkCore;

public class SearchDbContext(DbContextOptions<SearchDbContext> options) : DbContext(options)
{
    public DbSet<Lead> Leads => Set<Lead>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.ApplyConfiguration(new LeadConfiguration());
        base.OnModelCreating(modelBuilder);
    }
}
