// Read-side repository for global coverages. Interface in Domain; implementation in
// Coverage.Infrastructure.
public interface IGlobalCoverageQueryRepository
{
    Task<IReadOnlyList<GlobalCoverage>> GetAll(CancellationToken cancellationToken = default);
}
