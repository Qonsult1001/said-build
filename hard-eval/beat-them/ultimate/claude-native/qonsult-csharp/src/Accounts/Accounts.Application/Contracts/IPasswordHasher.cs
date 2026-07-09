// Port for password hashing/verification. Implemented in Accounts.Infrastructure (no crypto in the
// Application layer).
public interface IPasswordHasher
{
    string Hash(string password);

    bool Verify(string password, string passwordHash);
}
