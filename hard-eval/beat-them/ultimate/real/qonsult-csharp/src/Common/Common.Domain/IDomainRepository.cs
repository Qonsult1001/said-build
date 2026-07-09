// Common.Domain — repository abstraction shared by every context.

public interface IDomainRepository<TEntity> where TEntity : class, IAggregateRoot
{
    Task Save(TEntity entity, CancellationToken cancellationToken = default);

    Task<TEntity?> Find(Guid id, CancellationToken cancellationToken = default);
}

// Marker for read-side query repositories (no write contract).
public interface IQueryRepository { }
