using System.ComponentModel.DataAnnotations.Schema;

// Base type for all aggregate roots and entities. No namespace — implicit global
// namespace is the template convention (a `namespace` declaration is a bug here).
public abstract class Entity
{
    private readonly List<IDomainEvent> events = new();

    public Guid Id { get; private set; } = Guid.NewGuid();

    [NotMapped]
    public IReadOnlyCollection<IDomainEvent> Events => events.AsReadOnly();

    protected void RaiseEvent(IDomainEvent domainEvent) => events.Add(domainEvent);

    public void ClearEvents() => events.Clear();

    public override bool Equals(object? obj)
        => obj is Entity other && GetType() == other.GetType() && Id == other.Id;

    public override int GetHashCode() => HashCode.Combine(GetType(), Id);
}
