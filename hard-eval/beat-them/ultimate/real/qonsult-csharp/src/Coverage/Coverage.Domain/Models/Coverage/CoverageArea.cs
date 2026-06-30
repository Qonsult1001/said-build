// Coverage.Domain — CoverageArea entity, part of the GlobalCoverage aggregate.

public class CoverageArea : Entity
{
    private const double EarthRadiusKm = 6371.0;

    internal CoverageArea(Guid coverageId, string supplierName, double latitude, double longitude, double radiusKm)
    {
        if (string.IsNullOrWhiteSpace(supplierName))
        {
            throw new ArgumentException("Supplier name is required.", nameof(supplierName));
        }

        CoverageId = coverageId;
        SupplierName = supplierName.Trim();
        Latitude = latitude;
        Longitude = longitude;
        RadiusKm = radiusKm;
    }

    private CoverageArea() { } // EF

    public Guid CoverageId { get; private set; }
    public string SupplierName { get; private set; } = default!;
    public double Latitude { get; private set; }
    public double Longitude { get; private set; }
    public double RadiusKm { get; private set; }

    public bool Contains(double latitude, double longitude)
        => HaversineKm(Latitude, Longitude, latitude, longitude) <= RadiusKm;

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
