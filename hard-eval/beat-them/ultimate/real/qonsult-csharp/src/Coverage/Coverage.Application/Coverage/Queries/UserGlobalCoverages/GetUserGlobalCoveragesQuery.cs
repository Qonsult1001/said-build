// Coverage.Application — UserGlobalCoverages query slice.
// Backs GET /coverage/userglobalcoverages/.

using FluentValidation;

public record GetUserGlobalCoveragesQuery(Guid UserId);

public record CoverageAreaDto(string SupplierName, double Latitude, double Longitude, double RadiusKm);

public record UserGlobalCoveragesResponse(Guid UserId, IReadOnlyList<CoverageAreaDto> Coverages);

public interface IGetUserGlobalCoveragesService
{
    Task<Result<UserGlobalCoveragesResponse>> GetCoverages(GetUserGlobalCoveragesQuery query, CancellationToken cancellationToken = default);
}

public class GetUserGlobalCoveragesService(
    IGlobalCoverageQueryRepository queryRepository) : IGetUserGlobalCoveragesService
{
    public async Task<Result<UserGlobalCoveragesResponse>> GetCoverages(
        GetUserGlobalCoveragesQuery query,
        CancellationToken cancellationToken = default)
    {
        // validate the request (handled by validator) / authorize on UserId
        // query via the read-side repository
        var areas = await queryRepository.ListAreasForUser(query.UserId, cancellationToken);

        // map to DTO and return the response
        var dtos = areas
            .Select(a => new CoverageAreaDto(a.SupplierName, a.Latitude, a.Longitude, a.RadiusKm))
            .ToList();

        return Result.Success(new UserGlobalCoveragesResponse(query.UserId, dtos));
    }
}

public class GetUserGlobalCoveragesQueryValidator : AbstractValidator<GetUserGlobalCoveragesQuery>
{
    public GetUserGlobalCoveragesQueryValidator()
    {
        RuleFor(x => x.UserId).NotEmpty();
    }
}
