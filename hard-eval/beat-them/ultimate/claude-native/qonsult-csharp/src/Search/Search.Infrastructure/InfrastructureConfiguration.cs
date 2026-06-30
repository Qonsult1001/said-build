using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

// DI wiring for the Search Infrastructure layer: DbContext, the two write repositories, the
// preferences read repository, and the domain factories.
public static class SearchInfrastructureConfiguration
{
    public static IServiceCollection AddSearchInfrastructure(
        this IServiceCollection services,
        IConfiguration configuration)
    {
        services.AddDbContext<SearchDbContext>(options =>
            options.UseSqlServer(configuration.GetConnectionString("SearchConnection")));

        return services
            .AddScoped<ISearchDomainRepository, SearchRepository>()
            .AddScoped<ILeadDomainRepository, LeadRepository>()
            .AddScoped<IUserPreferenceQueryRepository, UserPreferenceRepository>()
            .AddScoped<ISearchFactory, SearchFactory>()
            .AddScoped<ILeadFactory, LeadFactory>();
    }
}
