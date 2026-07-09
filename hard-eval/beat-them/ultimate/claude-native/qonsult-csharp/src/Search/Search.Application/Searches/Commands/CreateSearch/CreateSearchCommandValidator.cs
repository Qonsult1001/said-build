using FluentValidation;

// FluentValidation rules for the create-search payload.
public class CreateSearchCommandValidator : AbstractValidator<CreateSearchCommand>
{
    public CreateSearchCommandValidator()
    {
        RuleFor(c => c.MapcheKey).NotEmpty();
        RuleFor(c => c.FormattedAddress).NotEmpty();
        RuleFor(c => c.LocationLat).InclusiveBetween(-90, 90);
        RuleFor(c => c.LocationLng).InclusiveBetween(-180, 180);
    }
}
