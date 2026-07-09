// Coverage.Infrastructure — DI wiring.

using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

public static class CoverageInfrastructureConfiguration
{
    public static IServiceCollection AddCoverageInfrastructure(
        this IServiceCollection services,
        IConfiguration configuration)
    {
        services.AddDbContext<CoverageDbContext>(options =>
            options.UseSqlServer(configuration.GetConnectionString("Coverage")));

        services.AddScoped<IGlobalCoverageDomainRepository, GlobalCoverageRepository>();
        services.AddScoped<IGlobalCoverageQueryRepository, GlobalCoverageRepository>();
        services.AddScoped<IGlobalCoverageFactory, GlobalCoverageFactory>();

        return services;
    }
}
