using FluentValidation;

// FluentValidation rules for the login payload; limits pulled from UserModelConstants.
public class LoginCommandValidator : AbstractValidator<LoginCommand>
{
    public LoginCommandValidator()
    {
        RuleFor(c => c.Username)
            .NotEmpty()
            .MinimumLength(UserModelConstants.UsernameMinLength)
            .MaximumLength(UserModelConstants.UsernameMaxLength);

        RuleFor(c => c.Password)
            .NotEmpty()
            .MinimumLength(UserModelConstants.PasswordMinLength);
    }
}
