using Microsoft.EntityFrameworkCore;

// Read-side repository implementation for feature data.
public class FeatureDataRepository(ProductsDbContext context) : IFeatureDataQueryRepository
{
    public async Task<IReadOnlyList<FeatureDatum>> GetAll(CancellationToken cancellationToken = default)
        => await context.FeatureData
            .AsNoTracking()
            .Where(f => f.IsActive)
            .ToListAsync(cancellationToken);
}
