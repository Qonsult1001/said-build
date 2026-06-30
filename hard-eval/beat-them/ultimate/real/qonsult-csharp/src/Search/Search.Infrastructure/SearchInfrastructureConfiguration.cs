// Search.Infrastructure — DI wiring (persistence + cross-context HTTP client).

using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

public static class SearchInfrastructureConfiguration
{
    public static IServiceCollection AddSearchInfrastructure(
        this IServiceCollection services,
        IConfiguration configuration)
    {
        services.AddDbContext<SearchDbContext>(options =>
            options.UseSqlServer(configuration.GetConnectionString("Search")));

        services.AddScoped<ILeadDomainRepository, LeadRepository>();
        services.AddScoped<ILeadQueryRepository, LeadRepository>();
        services.AddScoped<ILeadFactory, LeadFactory>();

        var coverageBaseUrl = configuration["Services:Coverage:BaseUrl"] ?? "http://localhost/";
        services.AddHttpClient<ICoverageLookup, CoverageHttpLookup>(client =>
            client.BaseAddress = new Uri(coverageBaseUrl));

        return services;
    }
}
