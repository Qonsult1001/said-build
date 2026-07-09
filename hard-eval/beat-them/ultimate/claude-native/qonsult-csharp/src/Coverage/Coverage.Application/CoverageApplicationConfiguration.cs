using System.Reflection;
using FluentValidation;
using Microsoft.Extensions.DependencyInjection;

// DI wiring for the Coverage Application layer.
public static class CoverageApplicationConfiguration
{
    public static IServiceCollection AddCoverageApplication(this IServiceCollection services)
    {
        services.AddValidatorsFromAssembly(Assembly.GetExecutingAssembly());

        return services
            .AddScoped<IGetUserGlobalCoveragesService, GetUserGlobalCoveragesService>();
    }
}
