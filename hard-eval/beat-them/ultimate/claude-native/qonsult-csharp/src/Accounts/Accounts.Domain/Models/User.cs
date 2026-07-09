// Accounts context — User aggregate root. Owns authentication identity (credentials, auth token) and
// the profile fields the frontend reads back via /userprofile. Private setters; behaviour validates
// invariants in-aggregate and returns `this`.
public class User : Entity, IAggregateRoot
{
    private User()
    {
    }

    public User(string username, string passwordHash, string email)
    {
        ValidateUsername(username);
        ValidatePasswordHash(passwordHash);

        Username = username;
        PasswordHash = passwordHash;
        Email = email ?? string.Empty;
        RaiseEvent(new UserRegisteredEvent(Id, username));
    }

    public string Username { get; private set; } = string.Empty;

    public string PasswordHash { get; private set; } = string.Empty;

    public string Email { get; private set; } = string.Empty;

    public string? FirstName { get; private set; }

    public string? LastName { get; private set; }

    public string? MapcheKey { get; private set; }

    public string? AuthToken { get; private set; }

    public User WithProfile(string? firstName, string? lastName, string? mapcheKey)
    {
        FirstName = firstName;
        LastName = lastName;
        MapcheKey = mapcheKey;
        return this;
    }

    // Issues a fresh opaque auth token after a successful credential check. The frontend stores it as
    // `Token {auth_token}` and sends it on every subsequent request.
    public User IssueToken(string token)
    {
        if (string.IsNullOrWhiteSpace(token))
        {
            throw new InvalidOperationException("Auth token must not be empty.");
        }

        AuthToken = token;
        return this;
    }

    private static void ValidateUsername(string username)
    {
        if (string.IsNullOrWhiteSpace(username))
        {
            throw new InvalidOperationException("Username is required.");
        }

        if (username.Length > UserModelConstants.UsernameMaxLength)
        {
            throw new InvalidOperationException(
                $"Username must be at most {UserModelConstants.UsernameMaxLength} characters.");
        }
    }

    private static void ValidatePasswordHash(string passwordHash)
    {
        if (string.IsNullOrWhiteSpace(passwordHash))
        {
            throw new InvalidOperationException("Password hash is required.");
        }
    }
}
