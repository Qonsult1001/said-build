// Search.Application — DI registration, chained off AddCommonApplication.

using System.Reflection;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

public static class SearchApplicationConfiguration
{
    public static IServiceCollection AddSearchApplication(
        this IServiceCollection services,
        IConfiguration configuration)
        => services
            .AddCommonApplication(configuration, Assembly.GetExecutingAssembly())
            .AddScoped<ISearchCoverageService, SearchCoverageService>()
            .AddScoped<ICaptureLeadService, CaptureLeadService>();
}
