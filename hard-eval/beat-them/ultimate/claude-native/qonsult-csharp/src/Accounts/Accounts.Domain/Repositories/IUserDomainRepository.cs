// Write-side repository for the User aggregate. Interface lives in Domain; the implementation lives in
// Accounts.Infrastructure.
public interface IUserDomainRepository : IDomainRepository<User>
{
    Task<User?> FindByUsername(string username, CancellationToken cancellationToken = default);
}
