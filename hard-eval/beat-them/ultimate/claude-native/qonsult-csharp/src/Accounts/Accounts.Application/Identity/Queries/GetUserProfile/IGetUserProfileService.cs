// Use-case contract for reading a user profile by mapche_key.
public interface IGetUserProfileService
{
    Task<Result<UserProfileResponse>> GetProfile(GetUserProfileQuery query, CancellationToken cancellationToken = default);
}
