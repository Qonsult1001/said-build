using System.Reflection;
using FluentValidation;
using Microsoft.Extensions.DependencyInjection;

// DI wiring for the Products Application layer.
public static class ProductsApplicationConfiguration
{
    public static IServiceCollection AddProductsApplication(this IServiceCollection services)
    {
        services.AddValidatorsFromAssembly(Assembly.GetExecutingAssembly());

        return services
            .AddScoped<IGetFeatureDataService, GetFeatureDataService>()
            .AddScoped<IGetProductsService, GetProductsService>();
    }
}
