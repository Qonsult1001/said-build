// Marker for domain events raised inside aggregates. Cross-context contracts also derive from this
// and live in Common.Domain so neither context references the other's projects.
public interface IDomainEvent
{
}
