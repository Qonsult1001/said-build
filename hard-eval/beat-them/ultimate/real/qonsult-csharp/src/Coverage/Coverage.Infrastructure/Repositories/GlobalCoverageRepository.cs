// Coverage.Infrastructure — implements both domain and query repositories.

using Microsoft.EntityFrameworkCore;

public class GlobalCoverageRepository(CoverageDbContext dbContext)
    : IGlobalCoverageDomainRepository, IGlobalCoverageQueryRepository
{
    public async Task Save(GlobalCoverage entity, CancellationToken cancellationToken = default)
    {
        var existing = await dbContext.Coverages
            .Include(c => c.Areas)
            .FirstOrDefaultAsync(c => c.Id == entity.Id, cancellationToken);

        if (existing is null)
        {
            await dbContext.Coverages.AddAsync(entity, cancellationToken);
        }
        else
        {
            // Areas are owned children; EF tracks adds against the loaded graph.
            dbContext.Entry(existing).CurrentValues.SetValues(entity);
        }

        await dbContext.SaveChangesAsync(cancellationToken);
    }

    public async Task<GlobalCoverage?> Find(Guid id, CancellationToken cancellationToken = default)
        => await dbContext.Coverages
            .Include(c => c.Areas)
            .FirstOrDefaultAsync(c => c.Id == id, cancellationToken);

    public async Task<GlobalCoverage?> FindByUser(Guid userId, CancellationToken cancellationToken = default)
        => await dbContext.Coverages
            .Include(c => c.Areas) // without Include, AddArea sees an empty collection and duplicates
            .FirstOrDefaultAsync(c => c.UserId == userId, cancellationToken);

    public async Task<IReadOnlyList<CoverageArea>> ListAreasForUser(Guid userId, CancellationToken cancellationToken = default)
        => await dbContext.CoverageAreas
            .AsNoTracking()
            .Where(a => dbContext.Coverages.Any(c => c.Id == a.CoverageId && c.UserId == userId))
            .ToListAsync(cancellationToken);
}
