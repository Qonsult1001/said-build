// Accounts.Application — DI registration, chained off AddCommonApplication.

using System.Reflection;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

public static class AccountsApplicationConfiguration
{
    public static IServiceCollection AddAccountsApplication(
        this IServiceCollection services,
        IConfiguration configuration)
        => services
            .AddCommonApplication(configuration, Assembly.GetExecutingAssembly())
            .AddScoped<IRegisterAccountService, RegisterAccountService>()
            .AddScoped<ILoginService, LoginService>();
}
