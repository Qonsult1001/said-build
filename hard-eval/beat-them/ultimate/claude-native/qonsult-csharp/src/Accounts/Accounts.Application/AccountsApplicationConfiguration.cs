using System.Reflection;
using FluentValidation;
using Microsoft.Extensions.DependencyInjection;

// DI wiring for the Accounts Application layer. Registers each use-case service and the
// FluentValidation validators in this assembly.
public static class AccountsApplicationConfiguration
{
    public static IServiceCollection AddAccountsApplication(this IServiceCollection services)
    {
        services.AddValidatorsFromAssembly(Assembly.GetExecutingAssembly());

        return services
            .AddScoped<ILoginService, LoginService>()
            .AddScoped<IGetUserProfileService, GetUserProfileService>();
    }
}
