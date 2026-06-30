// Fluent factory contract for the User aggregate. The aggregate is never `new`'d directly from the
// Application layer — it goes through Build().
public interface IUserFactory
{
    IUserFactory WithUsername(string username);

    IUserFactory WithPasswordHash(string passwordHash);

    IUserFactory WithEmail(string email);

    IUserFactory WithProfile(string? firstName, string? lastName, string? mapcheKey);

    User Build();
}
