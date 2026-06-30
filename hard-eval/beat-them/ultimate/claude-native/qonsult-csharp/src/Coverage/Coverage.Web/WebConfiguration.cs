using Microsoft.Extensions.DependencyInjection;

// Web-layer registration hook for the Coverage context.
public static class CoverageWebConfiguration
{
    public static IServiceCollection AddCoverageWebComponents(this IServiceCollection services)
        => services;
}
