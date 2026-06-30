// Accounts.Infrastructure — EF mapping for the Account aggregate.

using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Metadata.Builders;

public class AccountConfiguration : IEntityTypeConfiguration<Account>
{
    public void Configure(EntityTypeBuilder<Account> builder)
    {
        builder.HasKey(a => a.Id);

        builder.Property(a => a.Email)
            .IsRequired()
            .HasMaxLength(CommonModelConstants.Common.MaxEmailLength);

        builder.HasIndex(a => a.Email).IsUnique();

        builder.Property(a => a.PasswordHash).IsRequired();

        builder.Property(a => a.DisplayName)
            .IsRequired()
            .HasMaxLength(CommonModelConstants.Common.MaxNameLength);

        builder.Property(a => a.IsActive);
        builder.Property(a => a.LastLoginUtc);

        builder.Ignore(a => a.Events);
    }
}
