// Accounts.Domain — the Account aggregate root. Backs /accounts/token/login.

public class Account : Entity, IAggregateRoot
{
    // EF / factory constructor.
    internal Account(string email, string passwordHash, string displayName)
    {
        ValidateEmail(email);
        ValidateDisplayName(displayName);

        Email = email.Trim().ToLowerInvariant();
        PasswordHash = passwordHash;
        DisplayName = displayName.Trim();
        IsActive = true;

        RaiseEvent(new AccountRegisteredEvent(Id, Email));
    }

    private Account() { } // EF

    public string Email { get; private set; } = default!;
    public string PasswordHash { get; private set; } = default!;
    public string DisplayName { get; private set; } = default!;
    public bool IsActive { get; private set; }
    public DateTime? LastLoginUtc { get; private set; }

    public Account RecordLogin()
    {
        if (!IsActive)
        {
            throw new InvalidOperationException("Cannot log into a deactivated account.");
        }

        LastLoginUtc = DateTime.UtcNow;
        RaiseEvent(new AccountLoggedInEvent(Id, Email, LastLoginUtc.Value));
        return this;
    }

    public Account Deactivate()
    {
        IsActive = false;
        return this;
    }

    private static void ValidateEmail(string email)
    {
        if (string.IsNullOrWhiteSpace(email) || !email.Contains('@'))
        {
            throw new ArgumentException("A valid email is required.", nameof(email));
        }
    }

    private static void ValidateDisplayName(string displayName)
    {
        if (string.IsNullOrWhiteSpace(displayName))
        {
            throw new ArgumentException("Display name is required.", nameof(displayName));
        }
    }
}
