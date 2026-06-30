// Port for issuing the opaque auth token returned by /accounts/token/login. Implemented in
// Accounts.Infrastructure.
public interface ITokenGenerator
{
    string Generate(User user);
}
