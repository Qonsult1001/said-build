using FluentValidation;

// Read the profile for a mapche_key. Query shape: validate -> read via query repository -> map to read
// DTO -> return Result<T>. Uses the read-side repository, never the aggregate's write path.
public class GetUserProfileService(
    IUserQueryRepository users,
    IValidator<GetUserProfileQuery> validator) : IGetUserProfileService
{
    public async Task<Result<UserProfileResponse>> GetProfile(
        GetUserProfileQuery query,
        CancellationToken cancellationToken = default)
    {
        var validation = await validator.ValidateAsync(query, cancellationToken);
        if (!validation.IsValid)
        {
            return Result<UserProfileResponse>.Failure(validation.Errors.Select(e => e.ErrorMessage));
        }

        var user = await users.FindByMapcheKey(query.MapcheKey, cancellationToken);
        if (user is null)
        {
            return Result<UserProfileResponse>.Failure("No profile found for the supplied mapche_key.");
        }

        return new UserProfileResponse(
            user.Id,
            user.Username,
            user.Email,
            user.FirstName,
            user.LastName,
            user.MapcheKey);
    }
}
