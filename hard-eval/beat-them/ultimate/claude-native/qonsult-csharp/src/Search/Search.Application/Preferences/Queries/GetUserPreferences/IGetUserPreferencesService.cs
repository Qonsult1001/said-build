// Use-case contract for listing user preferences.
public interface IGetUserPreferencesService
{
    Task<Result<IReadOnlyList<UserPreferenceResponse>>> GetPreferences(
        GetUserPreferencesQuery query,
        CancellationToken cancellationToken = default);
}
