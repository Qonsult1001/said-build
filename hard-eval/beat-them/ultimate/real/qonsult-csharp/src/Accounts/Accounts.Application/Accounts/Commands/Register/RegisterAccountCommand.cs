// Accounts.Application — Register slice: command, service, validator, response DTO.

using FluentValidation;

public record RegisterAccountCommand(string Email, string Password, string DisplayName);

public record RegisterAccountResponse(Guid AccountId, string Email);

public interface IRegisterAccountService
{
    Task<Result<RegisterAccountResponse>> Register(RegisterAccountCommand command, CancellationToken cancellationToken = default);
}

public class RegisterAccountService(
    IAccountDomainRepository repository,
    IAccountQueryRepository queryRepository,
    IAccountFactory factory,
    IPasswordHasher passwordHasher) : IRegisterAccountService
{
    public async Task<Result<RegisterAccountResponse>> Register(
        RegisterAccountCommand command,
        CancellationToken cancellationToken = default)
    {
        // validate the request
        if (await queryRepository.EmailExists(command.Email, cancellationToken))
        {
            return Result.Failure<RegisterAccountResponse>("An account with this email already exists.");
        }

        // query or persist via repository
        var account = factory
            .WithEmail(command.Email)
            .WithPasswordHash(passwordHasher.Hash(command.Password))
            .WithDisplayName(command.DisplayName)
            .Build();

        await repository.Save(account, cancellationToken);

        // map to DTO and return the response
        return Result.Success(new RegisterAccountResponse(account.Id, account.Email));
    }
}

public class RegisterAccountCommandValidator : AbstractValidator<RegisterAccountCommand>
{
    public RegisterAccountCommandValidator()
    {
        RuleFor(x => x.Email)
            .NotEmpty()
            .EmailAddress()
            .MaximumLength(CommonModelConstants.Common.MaxEmailLength);

        RuleFor(x => x.Password)
            .NotEmpty()
            .MinimumLength(8);

        RuleFor(x => x.DisplayName)
            .NotEmpty()
            .MaximumLength(CommonModelConstants.Common.MaxNameLength);
    }
}
