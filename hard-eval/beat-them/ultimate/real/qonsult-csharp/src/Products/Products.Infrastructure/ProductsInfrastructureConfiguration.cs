// Products.Infrastructure — DI wiring.

using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

public static class ProductsInfrastructureConfiguration
{
    public static IServiceCollection AddProductsInfrastructure(
        this IServiceCollection services,
        IConfiguration configuration)
    {
        services.AddDbContext<ProductsDbContext>(options =>
            options.UseSqlServer(configuration.GetConnectionString("Products")));

        services.AddScoped<IProductDomainRepository, ProductRepository>();
        services.AddScoped<IProductQueryRepository, ProductRepository>();
        services.AddScoped<IProductFactory, ProductFactory>();

        return services;
    }
}
