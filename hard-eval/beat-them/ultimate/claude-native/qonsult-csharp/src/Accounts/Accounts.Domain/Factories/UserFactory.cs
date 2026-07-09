// Internal fluent builder for User. Build() asserts the required fields are set, then news the
// aggregate (the only place that does).
internal class UserFactory : IUserFactory
{
    private string? username;
    private string? passwordHash;
    private string email = string.Empty;
    private string? firstName;
    private string? lastName;
    private string? mapcheKey;

    public IUserFactory WithUsername(string username)
    {
        this.username = username;
        return this;
    }

    public IUserFactory WithPasswordHash(string passwordHash)
    {
        this.passwordHash = passwordHash;
        return this;
    }

    public IUserFactory WithEmail(string email)
    {
        this.email = email;
        return this;
    }

    public IUserFactory WithProfile(string? firstName, string? lastName, string? mapcheKey)
    {
        this.firstName = firstName;
        this.lastName = lastName;
        this.mapcheKey = mapcheKey;
        return this;
    }

    public User Build()
    {
        if (string.IsNullOrWhiteSpace(username))
        {
            throw new InvalidOperationException("Cannot build a User without a username.");
        }

        if (string.IsNullOrWhiteSpace(passwordHash))
        {
            throw new InvalidOperationException("Cannot build a User without a password hash.");
        }

        return new User(username, passwordHash, email)
            .WithProfile(firstName, lastName, mapcheKey);
    }
}
