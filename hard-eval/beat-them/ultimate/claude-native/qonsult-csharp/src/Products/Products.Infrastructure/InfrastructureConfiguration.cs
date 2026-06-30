using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

// DI wiring for the Products Infrastructure layer.
public static class ProductsInfrastructureConfiguration
{
    public static IServiceCollection AddProductsInfrastructure(
        this IServiceCollection services,
        IConfiguration configuration)
    {
        services.AddDbContext<ProductsDbContext>(options =>
            options.UseSqlServer(configuration.GetConnectionString("ProductsConnection")));

        return services
            .AddScoped<IFeatureDataQueryRepository, FeatureDataRepository>()
            .AddScoped<IProductQueryRepository, ProductRepository>()
            .AddScoped<IProductFactory, ProductFactory>();
    }
}
