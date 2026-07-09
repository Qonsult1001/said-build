using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

// DI wiring for the Coverage Infrastructure layer.
public static class CoverageInfrastructureConfiguration
{
    public static IServiceCollection AddCoverageInfrastructure(
        this IServiceCollection services,
        IConfiguration configuration)
    {
        services.AddDbContext<CoverageDbContext>(options =>
            options.UseSqlServer(configuration.GetConnectionString("CoverageConnection")));

        return services
            .AddScoped<IGlobalCoverageQueryRepository, GlobalCoverageRepository>();
    }
}
