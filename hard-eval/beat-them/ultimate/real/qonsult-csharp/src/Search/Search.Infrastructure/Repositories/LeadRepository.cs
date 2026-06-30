// Search.Infrastructure — implements both domain and query repositories for Lead.

using Microsoft.EntityFrameworkCore;

public class LeadRepository(SearchDbContext dbContext)
    : ILeadDomainRepository, ILeadQueryRepository
{
    public async Task Save(Lead entity, CancellationToken cancellationToken = default)
    {
        var existing = await dbContext.Leads.FirstOrDefaultAsync(l => l.Id == entity.Id, cancellationToken);
        if (existing is null)
        {
            await dbContext.Leads.AddAsync(entity, cancellationToken);
        }
        else
        {
            dbContext.Entry(existing).CurrentValues.SetValues(entity);
        }

        await dbContext.SaveChangesAsync(cancellationToken);
    }

    public async Task<Lead?> Find(Guid id, CancellationToken cancellationToken = default)
        => await dbContext.Leads.FirstOrDefaultAsync(l => l.Id == id, cancellationToken);

    public async Task<IReadOnlyList<Lead>> ListByMapcheKey(string mapcheKey, CancellationToken cancellationToken = default)
        => await dbContext.Leads
            .AsNoTracking()
            .Where(l => l.MapcheKey == mapcheKey)
            .OrderByDescending(l => l.CapturedUtc)
            .ToListAsync(cancellationToken);
}
