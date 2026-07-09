using FluentValidation;

// Record a lead. Same canonical command shape as CreateSearch: validate -> build via factory ->
// persist via the domain repository -> map to a response DTO (carrying the generated reference_id).
public class CreateLeadService(
    ILeadDomainRepository leads,
    ILeadFactory factory,
    IValidator<CreateLeadCommand> validator) : ICreateLeadService
{
    public async Task<Result<CreateLeadResponse>> Create(
        CreateLeadCommand command,
        CancellationToken cancellationToken = default)
    {
        var validation = await validator.ValidateAsync(command, cancellationToken);
        if (!validation.IsValid)
        {
            return Result<CreateLeadResponse>.Failure(validation.Errors.Select(e => e.ErrorMessage));
        }

        var lead = factory
            .WithSearch(command.Search)
            .WithMapcheKey(command.MapcheKey)
            .WithProduct(command.Product, command.Mrc, command.Nrc)
            .WithCommercials(command.ProductType, command.SupplierName, command.Description)
            .WithParties(command.Business, command.Shopper)
            .Build();

        await leads.Save(lead, cancellationToken);

        return new CreateLeadResponse(lead.Id, lead.ReferenceId);
    }
}
