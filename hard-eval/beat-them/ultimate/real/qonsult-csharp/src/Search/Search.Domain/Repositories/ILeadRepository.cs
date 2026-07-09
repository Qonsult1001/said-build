// Search.Domain — repository interfaces only.

public interface ILeadDomainRepository : IDomainRepository<Lead>
{
}

public interface ILeadQueryRepository : IQueryRepository
{
    Task<IReadOnlyList<Lead>> ListByMapcheKey(string mapcheKey, CancellationToken cancellationToken = default);
}
