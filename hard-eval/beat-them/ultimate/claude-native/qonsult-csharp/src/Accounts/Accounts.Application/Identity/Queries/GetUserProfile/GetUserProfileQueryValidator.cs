using FluentValidation;

// Validates that a mapche_key was supplied before we hit the read store.
public class GetUserProfileQueryValidator : AbstractValidator<GetUserProfileQuery>
{
    public GetUserProfileQueryValidator()
    {
        RuleFor(q => q.MapcheKey).NotEmpty();
    }
}
