// Search.Domain — the Lead aggregate. Backs POST /mapche-api/v1/leads/.
// A captured prospect from a coverage search at a location.

public class Lead : Entity, IAggregateRoot
{
    internal Lead(string mapcheKey, string fullName, string contactNumber, double latitude, double longitude)
    {
        ValidateMapcheKey(mapcheKey);
        ValidateContact(fullName, contactNumber);

        MapcheKey = mapcheKey.Trim();
        FullName = fullName.Trim();
        ContactNumber = contactNumber.Trim();
        Latitude = latitude;
        Longitude = longitude;
        Status = LeadStatus.New;
        CapturedUtc = DateTime.UtcNow;

        RaiseEvent(new LeadCapturedEvent(Id, MapcheKey, ContactNumber));
    }

    private Lead() { } // EF

    public string MapcheKey { get; private set; } = default!;
    public string FullName { get; private set; } = default!;
    public string ContactNumber { get; private set; } = default!;
    public double Latitude { get; private set; }
    public double Longitude { get; private set; }
    public LeadStatus Status { get; private set; }
    public DateTime CapturedUtc { get; private set; }

    public Lead Qualify()
    {
        if (Status == LeadStatus.Converted)
        {
            throw new InvalidOperationException("A converted lead cannot be re-qualified.");
        }

        Status = LeadStatus.Qualified;
        return this;
    }

    public Lead Convert()
    {
        Status = LeadStatus.Converted;
        RaiseEvent(new LeadConvertedEvent(Id, MapcheKey));
        return this;
    }

    private static void ValidateMapcheKey(string mapcheKey)
    {
        if (string.IsNullOrWhiteSpace(mapcheKey))
        {
            throw new ArgumentException("MapcheKey is required.", nameof(mapcheKey));
        }
    }

    private static void ValidateContact(string fullName, string contactNumber)
    {
        if (string.IsNullOrWhiteSpace(fullName))
        {
            throw new ArgumentException("Full name is required.", nameof(fullName));
        }

        if (string.IsNullOrWhiteSpace(contactNumber))
        {
            throw new ArgumentException("Contact number is required.", nameof(contactNumber));
        }
    }
}

public enum LeadStatus
{
    New = 0,
    Qualified = 1,
    Converted = 2
}
