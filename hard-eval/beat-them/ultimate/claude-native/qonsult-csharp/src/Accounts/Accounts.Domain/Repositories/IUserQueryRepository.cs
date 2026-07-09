// Read-side repository for User. Returns aggregates for query services to project into read DTOs;
// implementation lives in Accounts.Infrastructure.
public interface IUserQueryRepository
{
    Task<User?> FindByMapcheKey(string mapcheKey, CancellationToken cancellationToken = default);

    Task<User?> FindByAuthToken(string authToken, CancellationToken cancellationToken = default);
}
