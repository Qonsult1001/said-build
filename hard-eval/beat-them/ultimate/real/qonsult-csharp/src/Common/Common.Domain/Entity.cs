// Common.Domain — shared kernel. No namespace (implicit global namespace per conventions).

public interface IAggregateRoot { }

public abstract class Entity
{
    private readonly List<IDomainEvent> _events = new();

    public Guid Id { get; protected set; } = Guid.NewGuid();

    public IReadOnlyCollection<IDomainEvent> Events => _events.AsReadOnly();

    protected void RaiseEvent(IDomainEvent domainEvent) => _events.Add(domainEvent);

    public void ClearEvents() => _events.Clear();
}

public interface IDomainEvent
{
    DateTime OccurredOnUtc { get; }
}

public abstract class DomainEvent : IDomainEvent
{
    public DateTime OccurredOnUtc { get; } = DateTime.UtcNow;
}
