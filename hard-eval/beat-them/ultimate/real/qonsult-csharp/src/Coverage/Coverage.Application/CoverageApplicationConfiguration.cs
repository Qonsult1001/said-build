// Coverage.Application — DI registration, chained off AddCommonApplication.

using System.Reflection;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

public static class CoverageApplicationConfiguration
{
    public static IServiceCollection AddCoverageApplication(
        this IServiceCollection services,
        IConfiguration configuration)
        => services
            .AddCommonApplication(configuration, Assembly.GetExecutingAssembly())
            .AddScoped<IAddCoverageAreaService, AddCoverageAreaService>()
            .AddScoped<IGetUserGlobalCoveragesService, GetUserGlobalCoveragesService>();
}
