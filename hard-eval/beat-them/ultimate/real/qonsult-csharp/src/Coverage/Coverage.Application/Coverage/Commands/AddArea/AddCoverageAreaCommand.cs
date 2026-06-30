// Coverage.Application — AddArea slice. Renders the standard 5-section blueprint.

using FluentValidation;

public record AddCoverageAreaCommand(Guid UserId, string SupplierName, double Latitude, double Longitude, double RadiusKm);

public record AddCoverageAreaResponse(Guid CoverageId, int AreaCount);

public interface IAddCoverageAreaService
{
    Task<Result<AddCoverageAreaResponse>> AddArea(AddCoverageAreaCommand command, CancellationToken cancellationToken = default);
}

public class AddCoverageAreaService(
    IGlobalCoverageDomainRepository repository,
    IGlobalCoverageFactory factory) : IAddCoverageAreaService
{
    public async Task<Result<AddCoverageAreaResponse>> AddArea(
        AddCoverageAreaCommand command,
        CancellationToken cancellationToken = default)
    {
        // validate the request / authorize: load (or create) this user's coverage aggregate
        var coverage = await repository.FindByUser(command.UserId, cancellationToken)
                       ?? factory.WithUserId(command.UserId).Build();

        // query or persist via repository
        coverage.AddArea(command.SupplierName, command.Latitude, command.Longitude, command.RadiusKm);
        await repository.Save(coverage, cancellationToken);

        // map to DTO and return the response
        return Result.Success(new AddCoverageAreaResponse(coverage.Id, coverage.Areas.Count));
    }
}

public class AddCoverageAreaCommandValidator : AbstractValidator<AddCoverageAreaCommand>
{
    public AddCoverageAreaCommandValidator()
    {
        RuleFor(x => x.UserId).NotEmpty();
        RuleFor(x => x.SupplierName).NotEmpty().MaximumLength(CommonModelConstants.Common.MaxNameLength);
        RuleFor(x => x.Latitude)
            .InclusiveBetween(CommonModelConstants.Common.MinLatitude, CommonModelConstants.Common.MaxLatitude);
        RuleFor(x => x.Longitude)
            .InclusiveBetween(CommonModelConstants.Common.MinLongitude, CommonModelConstants.Common.MaxLongitude);
        RuleFor(x => x.RadiusKm).GreaterThan(0);
    }
}
