// Accounts.Infrastructure — one class implements both the domain and query repositories.

using Microsoft.EntityFrameworkCore;

public class AccountRepository(AccountsDbContext dbContext)
    : IAccountDomainRepository, IAccountQueryRepository
{
    public async Task Save(Account entity, CancellationToken cancellationToken = default)
    {
        var existing = await dbContext.Accounts
            .FirstOrDefaultAsync(a => a.Id == entity.Id, cancellationToken);

        if (existing is null)
        {
            await dbContext.Accounts.AddAsync(entity, cancellationToken);
        }
        else
        {
            dbContext.Entry(existing).CurrentValues.SetValues(entity);
        }

        await dbContext.SaveChangesAsync(cancellationToken);
    }

    public async Task<Account?> Find(Guid id, CancellationToken cancellationToken = default)
        => await dbContext.Accounts.FirstOrDefaultAsync(a => a.Id == id, cancellationToken);

    public async Task<Account?> FindByEmail(string email, CancellationToken cancellationToken = default)
    {
        var normalized = email.Trim().ToLowerInvariant();
        return await dbContext.Accounts.FirstOrDefaultAsync(a => a.Email == normalized, cancellationToken);
    }

    public async Task<bool> EmailExists(string email, CancellationToken cancellationToken = default)
    {
        var normalized = email.Trim().ToLowerInvariant();
        return await dbContext.Accounts.AnyAsync(a => a.Email == normalized, cancellationToken);
    }
}
