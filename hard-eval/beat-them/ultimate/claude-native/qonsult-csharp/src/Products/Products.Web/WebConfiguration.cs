using Microsoft.Extensions.DependencyInjection;

// Web-layer registration hook for the Products context.
public static class ProductsWebConfiguration
{
    public static IServiceCollection AddProductsWebComponents(this IServiceCollection services)
        => services;
}
