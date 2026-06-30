// Search.Domain — fluent factory for Lead.

public interface ILeadFactory
{
    ILeadFactory WithMapcheKey(string mapcheKey);
    ILeadFactory WithContact(string fullName, string contactNumber);
    ILeadFactory WithLocation(double latitude, double longitude);
    Lead Build();
}

internal class LeadFactory : ILeadFactory
{
    private string? _mapcheKey;
    private string? _fullName;
    private string? _contactNumber;
    private double? _latitude;
    private double? _longitude;

    public ILeadFactory WithMapcheKey(string mapcheKey)
    {
        _mapcheKey = mapcheKey;
        return this;
    }

    public ILeadFactory WithContact(string fullName, string contactNumber)
    {
        _fullName = fullName;
        _contactNumber = contactNumber;
        return this;
    }

    public ILeadFactory WithLocation(double latitude, double longitude)
    {
        _latitude = latitude;
        _longitude = longitude;
        return this;
    }

    public Lead Build()
    {
        if (_mapcheKey is null || _fullName is null || _contactNumber is null ||
            _latitude is null || _longitude is null)
        {
            throw new InvalidOperationException("MapcheKey, contact and location are required to build a Lead.");
        }

        return new Lead(_mapcheKey, _fullName, _contactNumber, _latitude.Value, _longitude.Value);
    }
}
