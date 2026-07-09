using Microsoft.EntityFrameworkCore;

// Write-side repository implementation for the Search aggregate.
public class SearchRepository(SearchDbContext context) : ISearchDomainRepository
{
    public async Task<Search> Save(Search entity, CancellationToken cancellationToken = default)
    {
        if (context.Entry(entity).State == EntityState.Detached)
        {
            await context.Searches.AddAsync(entity, cancellationToken);
        }

        await context.SaveChangesAsync(cancellationToken);
        return entity;
    }

    public async Task<Search?> Find(Guid id, CancellationToken cancellationToken = default)
        => await context.Searches.FirstOrDefaultAsync(s => s.Id == id, cancellationToken);
}
