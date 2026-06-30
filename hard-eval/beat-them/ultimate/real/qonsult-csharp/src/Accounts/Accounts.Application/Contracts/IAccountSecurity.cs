// Accounts.Application — ports implemented in Infrastructure (JWT + password hashing).

public interface ITokenIssuer
{
    string IssueToken(Guid accountId, string email);
}

public interface IPasswordHasher
{
    string Hash(string password);
    bool Verify(string password, string passwordHash);
}
