using FluentValidation;

// FluentValidation rules for the create-lead payload.
public class CreateLeadCommandValidator : AbstractValidator<CreateLeadCommand>
{
    public CreateLeadCommandValidator()
    {
        RuleFor(c => c.MapcheKey).NotEmpty();
        RuleFor(c => c.Search).NotEqual(Guid.Empty);
        RuleFor(c => c.Product).NotEmpty();
        RuleFor(c => c.Mrc).GreaterThanOrEqualTo(0);
        RuleFor(c => c.Nrc).GreaterThanOrEqualTo(0);
    }
}
