using Microsoft.Extensions.DependencyInjection;

// Web-layer registration hook for the Search context.
public static class SearchWebConfiguration
{
    public static IServiceCollection AddSearchWebComponents(this IServiceCollection services)
        => services;
}
