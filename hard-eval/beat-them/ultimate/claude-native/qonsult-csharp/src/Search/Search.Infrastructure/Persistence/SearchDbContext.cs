using Microsoft.EntityFrameworkCore;

// EF Core context owning the Search schema (searches, leads, preferences).
public class SearchDbContext(DbContextOptions<SearchDbContext> options) : DbContext(options)
{
    public DbSet<Search> Searches => Set<Search>();

    public DbSet<Lead> Leads => Set<Lead>();

    public DbSet<UserPreference> UserPreferences => Set<UserPreference>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<Search>(search =>
        {
            search.HasKey(s => s.Id);
            search.Property(s => s.MapcheKey).IsRequired();
            search.Property(s => s.FormattedAddress).IsRequired();
            search.HasIndex(s => s.MapcheKey);
            search.Ignore(s => s.Events);
        });

        modelBuilder.Entity<Lead>(lead =>
        {
            lead.HasKey(l => l.Id);
            lead.Property(l => l.MapcheKey).IsRequired();
            lead.Property(l => l.Product).IsRequired();
            lead.Property(l => l.ReferenceId).IsRequired();
            lead.HasIndex(l => l.ReferenceId).IsUnique();
            lead.HasIndex(l => l.SearchId);
            lead.Ignore(l => l.Events);
        });

        modelBuilder.Entity<UserPreference>(preference =>
        {
            preference.HasKey(p => p.Id);
            preference.Property(p => p.MapcheKey).IsRequired();
            preference.HasIndex(p => p.MapcheKey);
            preference.HasIndex(p => p.BusinessId);
            preference.Ignore(p => p.Events);
        });

        base.OnModelCreating(modelBuilder);
    }
}
