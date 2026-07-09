// Search.Application — CaptureLead slice. Backs POST /mapche-api/v1/leads/.

using FluentValidation;

public record CaptureLeadCommand(string MapcheKey, string FullName, string ContactNumber, double Latitude, double Longitude);

public record CaptureLeadResponse(Guid LeadId, string Status);

public interface ICaptureLeadService
{
    Task<Result<CaptureLeadResponse>> Capture(CaptureLeadCommand command, CancellationToken cancellationToken = default);
}

public class CaptureLeadService(
    ILeadDomainRepository repository,
    ILeadFactory factory) : ICaptureLeadService
{
    public async Task<Result<CaptureLeadResponse>> Capture(
        CaptureLeadCommand command,
        CancellationToken cancellationToken = default)
    {
        // validate the request (validator) / authorize on MapcheKey
        // persist via repository
        var lead = factory
            .WithMapcheKey(command.MapcheKey)
            .WithContact(command.FullName, command.ContactNumber)
            .WithLocation(command.Latitude, command.Longitude)
            .Build();

        await repository.Save(lead, cancellationToken);

        // map to DTO and return the response
        return Result.Success(new CaptureLeadResponse(lead.Id, lead.Status.ToString()));
    }
}

public class CaptureLeadCommandValidator : AbstractValidator<CaptureLeadCommand>
{
    public CaptureLeadCommandValidator()
    {
        RuleFor(x => x.MapcheKey).NotEmpty();
        RuleFor(x => x.FullName).NotEmpty().MaximumLength(CommonModelConstants.Common.MaxNameLength);
        RuleFor(x => x.ContactNumber).NotEmpty().MaximumLength(32);
        RuleFor(x => x.Latitude)
            .InclusiveBetween(CommonModelConstants.Common.MinLatitude, CommonModelConstants.Common.MaxLatitude);
        RuleFor(x => x.Longitude)
            .InclusiveBetween(CommonModelConstants.Common.MinLongitude, CommonModelConstants.Common.MaxLongitude);
    }
}
