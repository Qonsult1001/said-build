// Products.Application — CreateProduct slice.

using FluentValidation;

public record CreateProductCommand(string Sku, string Name, decimal MonthlyPrice, string CurrencyCode, bool Featured);

public record CreateProductResponse(Guid ProductId, string Sku);

public interface ICreateProductService
{
    Task<Result<CreateProductResponse>> Create(CreateProductCommand command, CancellationToken cancellationToken = default);
}

public class CreateProductService(
    IProductDomainRepository repository,
    IProductQueryRepository queryRepository,
    IProductFactory factory) : ICreateProductService
{
    public async Task<Result<CreateProductResponse>> Create(
        CreateProductCommand command,
        CancellationToken cancellationToken = default)
    {
        // validate the request / authorize: SKU must be unique
        if (await repository.FindBySku(command.Sku, cancellationToken) is not null)
        {
            return Result.Failure<CreateProductResponse>($"A product with SKU '{command.Sku}' already exists.");
        }

        // persist via repository
        var product = factory
            .WithSku(command.Sku)
            .WithName(command.Name)
            .WithPrice(command.MonthlyPrice, command.CurrencyCode)
            .Build();

        if (command.Featured)
        {
            product.Feature();
        }

        await repository.Save(product, cancellationToken);

        // map to DTO and return the response
        return Result.Success(new CreateProductResponse(product.Id, product.Sku));
    }
}

public class CreateProductCommandValidator : AbstractValidator<CreateProductCommand>
{
    public CreateProductCommandValidator()
    {
        RuleFor(x => x.Sku).NotEmpty().MaximumLength(64);
        RuleFor(x => x.Name).NotEmpty().MaximumLength(CommonModelConstants.Common.MaxNameLength);
        RuleFor(x => x.MonthlyPrice).GreaterThanOrEqualTo(0);
        RuleFor(x => x.CurrencyCode).NotEmpty().Length(3);
    }
}
