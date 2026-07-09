// List all global coverages. Same query shape as Accounts' GetUserProfile: read via the query
// repository -> map each aggregate to a read DTO -> return Result<T>. No parameters to validate.
public class GetUserGlobalCoveragesService(
    IGlobalCoverageQueryRepository coverages) : IGetUserGlobalCoveragesService
{
    public async Task<Result<IReadOnlyList<GlobalCoverageResponse>>> GetCoverages(
        GetUserGlobalCoveragesQuery query,
        CancellationToken cancellationToken = default)
    {
        var all = await coverages.GetAll(cancellationToken);

        IReadOnlyList<GlobalCoverageResponse> response = all
            .Select(c => new GlobalCoverageResponse(c.Id, c.BusinessId, c.CountryName, c.CountryCode, c.Currency))
            .ToList();

        return Result<IReadOnlyList<GlobalCoverageResponse>>.SuccessWith(response);
    }
}
