// Search.Application — port to the Coverage context. Read-at-request-time integration.
// Implemented as a typed HTTP client in Search.Infrastructure/HttpServices/ — NEVER a project
// reference to Coverage.*.

public record CoverageSummary(string SupplierName, double Latitude, double Longitude, double RadiusKm);

public interface ICoverageLookup
{
    Task<IReadOnlyList<CoverageSummary>> GetSuppliersCovering(double latitude, double longitude, CancellationToken cancellationToken = default);
}
