// Coverage.Domain — the GlobalCoverage aggregate. Backs /coverage/userglobalcoverages/.
// A user's coverage footprint: the geographic areas where service is available.

public class GlobalCoverage : Entity, IAggregateRoot
{
    private readonly List<CoverageArea> _areas = new();

    internal GlobalCoverage(Guid userId)
    {
        if (userId == Guid.Empty)
        {
            throw new ArgumentException("UserId is required.", nameof(userId));
        }

        UserId = userId;
    }

    private GlobalCoverage() { } // EF

    public Guid UserId { get; private set; }
    public IReadOnlyCollection<CoverageArea> Areas => _areas.AsReadOnly();

    public GlobalCoverage AddArea(string supplierName, double latitude, double longitude, double radiusKm)
    {
        ValidateCoordinates(latitude, longitude);
        ValidateRadius(radiusKm);

        _areas.Add(new CoverageArea(Id, supplierName, latitude, longitude, radiusKm));
        RaiseEvent(new CoverageAreaAddedEvent(Id, UserId, supplierName));
        return this;
    }

    public bool Covers(double latitude, double longitude)
        => _areas.Any(a => a.Contains(latitude, longitude));

    private static void ValidateCoordinates(double latitude, double longitude)
    {
        if (latitude is < CommonModelConstants.Common.MinLatitude or > CommonModelConstants.Common.MaxLatitude)
        {
            throw new ArgumentOutOfRangeException(nameof(latitude));
        }

        if (longitude is < CommonModelConstants.Common.MinLongitude or > CommonModelConstants.Common.MaxLongitude)
        {
            throw new ArgumentOutOfRangeException(nameof(longitude));
        }
    }

    private static void ValidateRadius(double radiusKm)
    {
        if (radiusKm <= 0)
        {
            throw new ArgumentException("Coverage radius must be positive.", nameof(radiusKm));
        }
    }
}
