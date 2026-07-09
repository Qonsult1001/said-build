using Microsoft.EntityFrameworkCore;

// Read-side repository implementation for GlobalCoverage.
public class GlobalCoverageRepository(CoverageDbContext context) : IGlobalCoverageQueryRepository
{
    public async Task<IReadOnlyList<GlobalCoverage>> GetAll(CancellationToken cancellationToken = default)
        => await context.GlobalCoverages
            .AsNoTracking()
            .Where(c => c.IsActive)
            .ToListAsync(cancellationToken);
}
