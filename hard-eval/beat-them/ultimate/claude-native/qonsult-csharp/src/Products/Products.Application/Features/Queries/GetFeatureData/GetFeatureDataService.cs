// List feature data. Same query shape as Coverage's GetUserGlobalCoverages: read via the query
// repository -> map each aggregate to a read DTO -> return Result<T>.
public class GetFeatureDataService(
    IFeatureDataQueryRepository features) : IGetFeatureDataService
{
    public async Task<Result<IReadOnlyList<FeatureDataResponse>>> GetFeatureData(
        GetFeatureDataQuery query,
        CancellationToken cancellationToken = default)
    {
        var all = await features.GetAll(cancellationToken);

        IReadOnlyList<FeatureDataResponse> response = all
            .Select(f => new FeatureDataResponse(f.Id, f.Name, f.Description, f.IsActive))
            .ToList();

        return Result<IReadOnlyList<FeatureDataResponse>>.SuccessWith(response);
    }
}
