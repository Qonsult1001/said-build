// Generic repository contract for aggregate roots. Concrete domain repositories derive from this in
// each context's Domain layer; the implementations live in that context's Infrastructure layer.
public interface IDomainRepository<TEntity>
    where TEntity : class, IAggregateRoot
{
    Task<TEntity> Save(TEntity entity, CancellationToken cancellationToken = default);

    Task<TEntity?> Find(Guid id, CancellationToken cancellationToken = default);
}
