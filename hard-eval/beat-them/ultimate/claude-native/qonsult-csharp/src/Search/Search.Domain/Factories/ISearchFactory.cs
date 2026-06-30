// Fluent factory contract for the Search aggregate.
public interface ISearchFactory
{
    ISearchFactory WithMapcheKey(string mapcheKey);

    ISearchFactory WithLocation(string formattedAddress, double lat, double lng);

    ISearchFactory WithAddressComponents(
        string? placeId,
        string countryName,
        string countryShort,
        string? province,
        string? town,
        string? suburb,
        string? postalCode);

    ISearchFactory WithGeoInfo(string? ip, string? city, string? country);

    ISearchFactory WithMobile(string? mobile);

    Search Build();
}
