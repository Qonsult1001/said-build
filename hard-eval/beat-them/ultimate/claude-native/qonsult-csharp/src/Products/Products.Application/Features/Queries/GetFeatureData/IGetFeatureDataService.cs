// Use-case contract for listing feature data.
public interface IGetFeatureDataService
{
    Task<Result<IReadOnlyList<FeatureDataResponse>>> GetFeatureData(
        GetFeatureDataQuery query,
        CancellationToken cancellationToken = default);
}
