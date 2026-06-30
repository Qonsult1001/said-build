using Microsoft.EntityFrameworkCore;

// EF Core context owning the Accounts schema. Lives in Infrastructure — never referenced by a
// controller; the Application layer talks to repository interfaces only.
public class AccountsDbContext(DbContextOptions<AccountsDbContext> options) : DbContext(options)
{
    public DbSet<User> Users => Set<User>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<User>(user =>
        {
            user.HasKey(u => u.Id);
            user.Property(u => u.Username).IsRequired().HasMaxLength(UserModelConstants.UsernameMaxLength);
            user.HasIndex(u => u.Username).IsUnique();
            user.Property(u => u.PasswordHash).IsRequired();
            user.Property(u => u.Email).HasMaxLength(UserModelConstants.EmailMaxLength);
            user.HasIndex(u => u.MapcheKey);
            user.Ignore(u => u.Events);
        });

        base.OnModelCreating(modelBuilder);
    }
}
