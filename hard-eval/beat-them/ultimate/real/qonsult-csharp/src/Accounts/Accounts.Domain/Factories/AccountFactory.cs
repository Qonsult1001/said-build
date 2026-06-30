// Accounts.Domain — fluent factory. The aggregate is never new'd directly in Application.

public interface IAccountFactory
{
    IAccountFactory WithEmail(string email);
    IAccountFactory WithPasswordHash(string passwordHash);
    IAccountFactory WithDisplayName(string displayName);
    Account Build();
}

internal class AccountFactory : IAccountFactory
{
    private string? _email;
    private string? _passwordHash;
    private string? _displayName;

    public IAccountFactory WithEmail(string email)
    {
        _email = email;
        return this;
    }

    public IAccountFactory WithPasswordHash(string passwordHash)
    {
        _passwordHash = passwordHash;
        return this;
    }

    public IAccountFactory WithDisplayName(string displayName)
    {
        _displayName = displayName;
        return this;
    }

    public Account Build()
    {
        if (_email is null || _passwordHash is null || _displayName is null)
        {
            throw new InvalidOperationException("Email, password hash and display name are required to build an Account.");
        }

        return new Account(_email, _passwordHash, _displayName);
    }
}
