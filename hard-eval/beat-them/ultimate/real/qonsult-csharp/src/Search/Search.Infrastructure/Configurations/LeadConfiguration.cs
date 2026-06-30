// Search.Infrastructure — EF mapping for the Lead aggregate.

using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Metadata.Builders;

public class LeadConfiguration : IEntityTypeConfiguration<Lead>
{
    public void Configure(EntityTypeBuilder<Lead> builder)
    {
        builder.HasKey(l => l.Id);
        builder.Property(l => l.MapcheKey).IsRequired().HasMaxLength(128);
        builder.HasIndex(l => l.MapcheKey);
        builder.Property(l => l.FullName).IsRequired().HasMaxLength(CommonModelConstants.Common.MaxNameLength);
        builder.Property(l => l.ContactNumber).IsRequired().HasMaxLength(32);
        builder.Property(l => l.Latitude);
        builder.Property(l => l.Longitude);
        builder.Property(l => l.Status).HasConversion<int>();
        builder.Property(l => l.CapturedUtc);
        builder.Ignore(l => l.Events);
    }
}
