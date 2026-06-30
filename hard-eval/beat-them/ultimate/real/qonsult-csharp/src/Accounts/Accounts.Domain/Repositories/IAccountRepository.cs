// Accounts.Domain — repository interfaces only. Implementations live in Infrastructure.

public interface IAccountDomainRepository : IDomainRepository<Account>
{
    Task<Account?> FindByEmail(string email, CancellationToken cancellationToken = default);
}

public interface IAccountQueryRepository : IQueryRepository
{
    Task<bool> EmailExists(string email, CancellationToken cancellationToken = default);
}
