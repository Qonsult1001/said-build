// Search context — Search aggregate root. Records a geocoded address the shopper searched, plus the
// IP/geo metadata the frontend collects. POST /mapche-api/v1/search/ creates one and reads back its id
// + resolved country. Private setters; invariants in-aggregate.
public class Search : Entity, IAggregateRoot
{
    private Search()
    {
    }

    public Search(string mapcheKey, string formattedAddress, double locationLat, double locationLng)
    {
        ValidateMapcheKey(mapcheKey);
        ValidateFormattedAddress(formattedAddress);

        MapcheKey = mapcheKey;
        FormattedAddress = formattedAddress;
        LocationLat = locationLat;
        LocationLng = locationLng;
        CreatedAt = DateTime.UtcNow;
    }

    public string MapcheKey { get; private set; } = string.Empty;

    public string FormattedAddress { get; private set; } = string.Empty;

    public double LocationLat { get; private set; }

    public double LocationLng { get; private set; }

    public string? PlaceId { get; private set; }

    public string CountryName { get; private set; } = string.Empty;

    public string CountryShort { get; private set; } = string.Empty;

    public string? Province { get; private set; }

    public string? Town { get; private set; }

    public string? Suburb { get; private set; }

    public string? PostalCode { get; private set; }

    public string? Mobile { get; private set; }

    public string? InfoIp { get; private set; }

    public string? InfoCity { get; private set; }

    public string? InfoCountry { get; private set; }

    public DateTime CreatedAt { get; private set; }

    // Attaches the geocoded address breakdown captured by the frontend.
    public Search WithAddressComponents(
        string? placeId,
        string countryName,
        string countryShort,
        string? province,
        string? town,
        string? suburb,
        string? postalCode)
    {
        PlaceId = placeId;
        CountryName = countryName ?? string.Empty;
        CountryShort = countryShort ?? string.Empty;
        Province = province;
        Town = town;
        Suburb = suburb;
        PostalCode = postalCode;
        return this;
    }

    // Attaches the IP-geolocation metadata captured at search time.
    public Search WithGeoInfo(string? ip, string? city, string? country)
    {
        InfoIp = ip;
        InfoCity = city;
        InfoCountry = country;
        return this;
    }

    // PATCH /mapche-api/v1/search/{id}/ updates only the mobile number.
    public Search SetMobile(string mobile)
    {
        Mobile = mobile;
        return this;
    }

    private static void ValidateMapcheKey(string mapcheKey)
    {
        if (string.IsNullOrWhiteSpace(mapcheKey))
        {
            throw new InvalidOperationException("mapche_key is required to record a search.");
        }
    }

    private static void ValidateFormattedAddress(string formattedAddress)
    {
        if (string.IsNullOrWhiteSpace(formattedAddress))
        {
            throw new InvalidOperationException("A formatted address is required.");
        }
    }
}
