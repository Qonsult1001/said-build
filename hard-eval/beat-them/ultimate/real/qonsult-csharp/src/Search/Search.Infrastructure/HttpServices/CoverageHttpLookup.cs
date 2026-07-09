// Search.Infrastructure — typed HTTP client implementing the ICoverageLookup port.
// Cross-context read: calls the Coverage context over HTTP, never via a project reference.

using System.Net.Http.Json;

public class CoverageHttpLookup(HttpClient httpClient) : ICoverageLookup
{
    public async Task<IReadOnlyList<CoverageSummary>> GetSuppliersCovering(
        double latitude,
        double longitude,
        CancellationToken cancellationToken = default)
    {
        var url = $"coverage/areas?latitude={latitude}&longitude={longitude}";
        var response = await httpClient.GetAsync(url, cancellationToken);
        response.EnsureSuccessStatusCode();

        var summaries = await response.Content
            .ReadFromJsonAsync<List<CoverageSummary>>(cancellationToken: cancellationToken);

        return summaries ?? new List<CoverageSummary>();
    }
}
