// Read-side repository for feature data. Interface in Domain; implementation in
// Products.Infrastructure.
public interface IFeatureDataQueryRepository
{
    Task<IReadOnlyList<FeatureDatum>> GetAll(CancellationToken cancellationToken = default);
}
