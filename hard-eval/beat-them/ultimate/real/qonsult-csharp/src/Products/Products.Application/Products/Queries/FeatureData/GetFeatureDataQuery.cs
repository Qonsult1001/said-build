// Products.Application — FeatureData query. Backs GET /products/featuredata/.

using FluentValidation;

public record GetFeatureDataQuery(string Sku);

public record FeatureDto(string Label, string Value);

public record FeatureDataResponse(string Sku, string ProductName, IReadOnlyList<FeatureDto> Features);

public interface IGetFeatureDataService
{
    Task<Result<FeatureDataResponse>> GetFeatureData(GetFeatureDataQuery query, CancellationToken cancellationToken = default);
}

public class GetFeatureDataService(IProductDomainRepository repository) : IGetFeatureDataService
{
    public async Task<Result<FeatureDataResponse>> GetFeatureData(
        GetFeatureDataQuery query,
        CancellationToken cancellationToken = default)
    {
        // validate the request (validator) / authorize (public)
        // query via repository
        var product = await repository.FindBySku(query.Sku, cancellationToken);
        if (product is null)
        {
            return Result.Failure<FeatureDataResponse>($"No product found for SKU '{query.Sku}'.");
        }

        // map to DTO and return the response
        var features = product.Features
            .Select(f => new FeatureDto(f.Label, f.Value))
            .ToList();

        return Result.Success(new FeatureDataResponse(product.Sku, product.Name, features));
    }
}

public class GetFeatureDataQueryValidator : AbstractValidator<GetFeatureDataQuery>
{
    public GetFeatureDataQueryValidator()
    {
        RuleFor(x => x.Sku).NotEmpty();
    }
}
