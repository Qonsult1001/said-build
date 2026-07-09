// Search.Application — Search slice. Backs GET /mapche-api/v1/search/.
// Reads cross-context Coverage data via the ICoverageLookup HTTP port.

using FluentValidation;

public record SearchCoverageQuery(double Latitude, double Longitude, string? MapcheKey);

public record SupplierResultDto(string SupplierName, double DistanceKm);

public record SearchCoverageResponse(double Latitude, double Longitude, IReadOnlyList<SupplierResultDto> Suppliers);

public interface ISearchCoverageService
{
    Task<Result<SearchCoverageResponse>> Search(SearchCoverageQuery query, CancellationToken cancellationToken = default);
}

public class SearchCoverageService(ICoverageLookup coverageLookup) : ISearchCoverageService
{
    private const double EarthRadiusKm = 6371.0;

    public async Task<Result<SearchCoverageResponse>> Search(
        SearchCoverageQuery query,
        CancellationToken cancellationToken = default)
    {
        // validate the request (validator) / authorize on MapcheKey at the controller
        // query the Coverage context via the HTTP port (cross-context, not a project ref)
        var suppliers = await coverageLookup.GetSuppliersCovering(query.Latitude, query.Longitude, cancellationToken);

        // map to DTO and return the response, sorted nearest-first
        var results = suppliers
            .Select(s => new SupplierResultDto(
                s.SupplierName,
                HaversineKm(query.Latitude, query.Longitude, s.Latitude, s.Longitude)))
            .OrderBy(r => r.DistanceKm)
            .ToList();

        return Result.Success(new SearchCoverageResponse(query.Latitude, query.Longitude, results));
    }

    private static double HaversineKm(double lat1, double lon1, double lat2, double lon2)
    {
        double ToRad(double deg) => deg * Math.PI / 180.0;
        var dLat = ToRad(lat2 - lat1);
        var dLon = ToRad(lon2 - lon1);
        var a = Math.Sin(dLat / 2) * Math.Sin(dLat / 2)
                + Math.Cos(ToRad(lat1)) * Math.Cos(ToRad(lat2))
                * Math.Sin(dLon / 2) * Math.Sin(dLon / 2);
        return EarthRadiusKm * 2 * Math.Asin(Math.Min(1.0, Math.Sqrt(a)));
    }
}

public class SearchCoverageQueryValidator : AbstractValidator<SearchCoverageQuery>
{
    public SearchCoverageQueryValidator()
    {
        RuleFor(x => x.Latitude)
            .InclusiveBetween(CommonModelConstants.Common.MinLatitude, CommonModelConstants.Common.MaxLatitude);
        RuleFor(x => x.Longitude)
            .InclusiveBetween(CommonModelConstants.Common.MinLongitude, CommonModelConstants.Common.MaxLongitude);
    }
}
