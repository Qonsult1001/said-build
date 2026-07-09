// Products.Application — DI registration, chained off AddCommonApplication.

using System.Reflection;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

public static class ProductsApplicationConfiguration
{
    public static IServiceCollection AddProductsApplication(
        this IServiceCollection services,
        IConfiguration configuration)
        => services
            .AddCommonApplication(configuration, Assembly.GetExecutingAssembly())
            .AddScoped<ICreateProductService, CreateProductService>()
            .AddScoped<IListProductsService, ListProductsService>()
            .AddScoped<IGetFeatureDataService, GetFeatureDataService>();
}
