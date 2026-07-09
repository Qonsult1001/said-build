// Coverage.Domain — repository interfaces only.

public interface IGlobalCoverageDomainRepository : IDomainRepository<GlobalCoverage>
{
    Task<GlobalCoverage?> FindByUser(Guid userId, CancellationToken cancellationToken = default);
}

public interface IGlobalCoverageQueryRepository : IQueryRepository
{
    Task<IReadOnlyList<CoverageArea>> ListAreasForUser(Guid userId, CancellationToken cancellationToken = default);
}
