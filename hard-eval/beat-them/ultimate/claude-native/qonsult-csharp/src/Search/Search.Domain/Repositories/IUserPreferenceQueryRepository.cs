// Read-side repository for user preferences. Optional mapche_key filter mirrors the
// ?mapche_key= query param the frontend may send.
public interface IUserPreferenceQueryRepository
{
    Task<IReadOnlyList<UserPreference>> GetByMapcheKey(string? mapcheKey, CancellationToken cancellationToken = default);
}
