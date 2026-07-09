// Accounts.Domain — context-local domain events.

public class AccountRegisteredEvent(Guid accountId, string email) : DomainEvent
{
    public Guid AccountId { get; } = accountId;
    public string Email { get; } = email;
}

public class AccountLoggedInEvent(Guid accountId, string email, DateTime loginUtc) : DomainEvent
{
    public Guid AccountId { get; } = accountId;
    public string Email { get; } = email;
    public DateTime LoginUtc { get; } = loginUtc;
}
