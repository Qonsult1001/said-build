using Microsoft.EntityFrameworkCore;

// Implements both the write-side (IUserDomainRepository) and read-side (IUserQueryRepository) contracts
// for User. Repository implementations live in Infrastructure; the interfaces live in Domain.
public class UserRepository(AccountsDbContext context) : IUserDomainRepository, IUserQueryRepository
{
    public async Task<User> Save(User entity, CancellationToken cancellationToken = default)
    {
        if (context.Entry(entity).State == EntityState.Detached)
        {
            await context.Users.AddAsync(entity, cancellationToken);
        }

        await context.SaveChangesAsync(cancellationToken);
        return entity;
    }

    public async Task<User?> Find(Guid id, CancellationToken cancellationToken = default)
        => await context.Users.FirstOrDefaultAsync(u => u.Id == id, cancellationToken);

    public async Task<User?> FindByUsername(string username, CancellationToken cancellationToken = default)
        => await context.Users.FirstOrDefaultAsync(u => u.Username == username, cancellationToken);

    public async Task<User?> FindByMapcheKey(string mapcheKey, CancellationToken cancellationToken = default)
        => await context.Users.AsNoTracking().FirstOrDefaultAsync(u => u.MapcheKey == mapcheKey, cancellationToken);

    public async Task<User?> FindByAuthToken(string authToken, CancellationToken cancellationToken = default)
        => await context.Users.AsNoTracking().FirstOrDefaultAsync(u => u.AuthToken == authToken, cancellationToken);
}
