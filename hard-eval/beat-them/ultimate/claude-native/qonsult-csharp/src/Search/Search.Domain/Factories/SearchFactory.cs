// Internal fluent builder for the Search aggregate. Build() asserts required fields then news it.
internal class SearchFactory : ISearchFactory
{
    private string? mapcheKey;
    private string? formattedAddress;
    private double lat;
    private double lng;
    private string? placeId;
    private string countryName = string.Empty;
    private string countryShort = string.Empty;
    private string? province;
    private string? town;
    private string? suburb;
    private string? postalCode;
    private string? ip;
    private string? city;
    private string? infoCountry;
    private string? mobile;

    public ISearchFactory WithMapcheKey(string mapcheKey)
    {
        this.mapcheKey = mapcheKey;
        return this;
    }

    public ISearchFactory WithLocation(string formattedAddress, double lat, double lng)
    {
        this.formattedAddress = formattedAddress;
        this.lat = lat;
        this.lng = lng;
        return this;
    }

    public ISearchFactory WithAddressComponents(
        string? placeId,
        string countryName,
        string countryShort,
        string? province,
        string? town,
        string? suburb,
        string? postalCode)
    {
        this.placeId = placeId;
        this.countryName = countryName;
        this.countryShort = countryShort;
        this.province = province;
        this.town = town;
        this.suburb = suburb;
        this.postalCode = postalCode;
        return this;
    }

    public ISearchFactory WithGeoInfo(string? ip, string? city, string? country)
    {
        this.ip = ip;
        this.city = city;
        this.infoCountry = country;
        return this;
    }

    public ISearchFactory WithMobile(string? mobile)
    {
        this.mobile = mobile;
        return this;
    }

    public Search Build()
    {
        if (string.IsNullOrWhiteSpace(mapcheKey))
        {
            throw new InvalidOperationException("Cannot build a Search without a mapche_key.");
        }

        if (string.IsNullOrWhiteSpace(formattedAddress))
        {
            throw new InvalidOperationException("Cannot build a Search without a formatted address.");
        }

        var search = new Search(mapcheKey, formattedAddress, lat, lng)
            .WithAddressComponents(placeId, countryName, countryShort, province, town, suburb, postalCode)
            .WithGeoInfo(ip, city, infoCountry);

        if (!string.IsNullOrWhiteSpace(mobile))
        {
            search.SetMobile(mobile);
        }

        return search;
    }
}
