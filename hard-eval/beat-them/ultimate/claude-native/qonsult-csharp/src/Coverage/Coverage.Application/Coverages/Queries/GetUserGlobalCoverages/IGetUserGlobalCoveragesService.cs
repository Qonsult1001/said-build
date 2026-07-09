// Use-case contract for listing the user's global coverages.
public interface IGetUserGlobalCoveragesService
{
    Task<Result<IReadOnlyList<GlobalCoverageResponse>>> GetCoverages(
        GetUserGlobalCoveragesQuery query,
        CancellationToken cancellationToken = default);
}
