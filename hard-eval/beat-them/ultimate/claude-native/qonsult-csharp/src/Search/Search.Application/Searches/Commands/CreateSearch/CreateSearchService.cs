using FluentValidation;

// Record a search. Canonical command shape (same as Accounts' LoginService): validate -> build the
// aggregate via the factory -> persist via the domain repository -> map to a response DTO -> return.
public class CreateSearchService(
    ISearchDomainRepository searches,
    ISearchFactory factory,
    IValidator<CreateSearchCommand> validator) : ICreateSearchService
{
    public async Task<Result<CreateSearchResponse>> Create(
        CreateSearchCommand command,
        CancellationToken cancellationToken = default)
    {
        var validation = await validator.ValidateAsync(command, cancellationToken);
        if (!validation.IsValid)
        {
            return Result<CreateSearchResponse>.Failure(validation.Errors.Select(e => e.ErrorMessage));
        }

        var search = factory
            .WithMapcheKey(command.MapcheKey)
            .WithLocation(command.FormattedAddress, command.LocationLat, command.LocationLng)
            .WithAddressComponents(
                command.PlaceId,
                command.CountryName,
                command.CountryShort,
                command.Province,
                command.Town,
                command.Suburb,
                command.PostalCode)
            .WithGeoInfo(command.InfoIp, command.InfoCity, command.InfoCountry)
            .WithMobile(command.Mobile)
            .Build();

        await searches.Save(search, cancellationToken);

        return new CreateSearchResponse(search.Id, search.CountryName, search.CountryShort);
    }
}
