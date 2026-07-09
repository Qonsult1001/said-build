// List user preferences. Same query shape as Coverage/UserProfile: read via the query repository
// (optionally filtered by mapche_key) -> map each aggregate to a read DTO -> return Result<T>.
public class GetUserPreferencesService(
    IUserPreferenceQueryRepository preferences) : IGetUserPreferencesService
{
    public async Task<Result<IReadOnlyList<UserPreferenceResponse>>> GetPreferences(
        GetUserPreferencesQuery query,
        CancellationToken cancellationToken = default)
    {
        var rows = await preferences.GetByMapcheKey(query.MapcheKey, cancellationToken);

        IReadOnlyList<UserPreferenceResponse> response = rows
            .Select(p => new UserPreferenceResponse(
                p.Id, p.MapcheKey, p.BusinessId, p.ResellerId, p.ProviderName, p.IsEnabled))
            .ToList();

        return Result<IReadOnlyList<UserPreferenceResponse>>.SuccessWith(response);
    }
}
