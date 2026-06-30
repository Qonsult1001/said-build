// Accounts.Application — Login slice. Backs POST /accounts/token/login.

using FluentValidation;

public record LoginCommand(string Email, string Password);

public record LoginResponse(string Token, Guid AccountId, string DisplayName);

public interface ILoginService
{
    Task<Result<LoginResponse>> Login(LoginCommand command, CancellationToken cancellationToken = default);
}

public class LoginService(
    IAccountDomainRepository repository,
    IPasswordHasher passwordHasher,
    ITokenIssuer tokenIssuer) : ILoginService
{
    public async Task<Result<LoginResponse>> Login(LoginCommand command, CancellationToken cancellationToken = default)
    {
        // validate the request / authorize: credentials must match an active account
        var account = await repository.FindByEmail(command.Email, cancellationToken);
        if (account is null || !passwordHasher.Verify(command.Password, account.PasswordHash))
        {
            return Result.Failure<LoginResponse>("Invalid email or password.");
        }

        // persist the login on the aggregate via repository
        account.RecordLogin();
        await repository.Save(account, cancellationToken);

        // map to DTO and return the response (the issued token)
        var token = tokenIssuer.IssueToken(account.Id, account.Email);
        return Result.Success(new LoginResponse(token, account.Id, account.DisplayName));
    }
}

public class LoginCommandValidator : AbstractValidator<LoginCommand>
{
    public LoginCommandValidator()
    {
        RuleFor(x => x.Email).NotEmpty().EmailAddress();
        RuleFor(x => x.Password).NotEmpty();
    }
}
