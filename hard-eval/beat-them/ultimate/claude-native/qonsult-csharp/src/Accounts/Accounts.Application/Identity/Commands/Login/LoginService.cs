using FluentValidation;

// Authenticate a user: validate the payload, verify credentials, issue + persist a token, return the
// token DTO. This is the canonical command shape — validate -> authorize -> persist via repository ->
// map to DTO -> return Result<T>.
public class LoginService(
    IUserDomainRepository users,
    IPasswordHasher passwordHasher,
    ITokenGenerator tokenGenerator,
    IValidator<LoginCommand> validator) : ILoginService
{
    public async Task<Result<LoginResponse>> Login(
        LoginCommand command,
        CancellationToken cancellationToken = default)
    {
        // 1. validate the request
        var validation = await validator.ValidateAsync(command, cancellationToken);
        if (!validation.IsValid)
        {
            return Result<LoginResponse>.Failure(validation.Errors.Select(e => e.ErrorMessage));
        }

        // 2. authorize — load the user and verify the password
        var user = await users.FindByUsername(command.Username, cancellationToken);
        if (user is null || !passwordHasher.Verify(command.Password, user.PasswordHash))
        {
            return Result<LoginResponse>.Failure("Invalid username or password.");
        }

        // 3. persist via repository — issue and store a fresh auth token on the aggregate
        user.IssueToken(tokenGenerator.Generate(user));
        await users.Save(user, cancellationToken);

        // 4. map to DTO and return
        return new LoginResponse(user.AuthToken!);
    }
}
