using Microsoft.EntityFrameworkCore;

// Read-side repository implementation for user preferences, with an optional mapche_key filter.
public class UserPreferenceRepository(SearchDbContext context) : IUserPreferenceQueryRepository
{
    public async Task<IReadOnlyList<UserPreference>> GetByMapcheKey(
        string? mapcheKey,
        CancellationToken cancellationToken = default)
    {
        var query = context.UserPreferences.AsNoTracking();

        if (!string.IsNullOrWhiteSpace(mapcheKey))
        {
            query = query.Where(p => p.MapcheKey == mapcheKey);
        }

        return await query.ToListAsync(cancellationToken);
    }
}
