// Context-local domain event raised when a User aggregate is first created.
public class UserRegisteredEvent : IDomainEvent
{
    public UserRegisteredEvent(Guid userId, string username)
    {
        UserId = userId;
        Username = username;
    }

    public Guid UserId { get; }

    public string Username { get; }
}
