using Microsoft.EntityFrameworkCore;

// Write-side repository implementation for the Lead aggregate.
public class LeadRepository(SearchDbContext context) : ILeadDomainRepository
{
    public async Task<Lead> Save(Lead entity, CancellationToken cancellationToken = default)
    {
        if (context.Entry(entity).State == EntityState.Detached)
        {
            await context.Leads.AddAsync(entity, cancellationToken);
        }

        await context.SaveChangesAsync(cancellationToken);
        return entity;
    }

    public async Task<Lead?> Find(Guid id, CancellationToken cancellationToken = default)
        => await context.Leads.FirstOrDefaultAsync(l => l.Id == id, cancellationToken);
}
