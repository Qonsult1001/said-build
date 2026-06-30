using System.Reflection;
using FluentValidation;
using Microsoft.Extensions.DependencyInjection;

// DI wiring for the Search Application layer: the two command services and the preferences query
// service, plus all validators in this assembly.
public static class SearchApplicationConfiguration
{
    public static IServiceCollection AddSearchApplication(this IServiceCollection services)
    {
        services.AddValidatorsFromAssembly(Assembly.GetExecutingAssembly());

        return services
            .AddScoped<ICreateSearchService, CreateSearchService>()
            .AddScoped<ICreateLeadService, CreateLeadService>()
            .AddScoped<IGetUserPreferencesService, GetUserPreferencesService>();
    }
}
