using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

// DI wiring for the Accounts Infrastructure layer: DbContext, repository (bound to both contracts),
// and the crypto/token services that satisfy the Application ports.
public static class AccountsInfrastructureConfiguration
{
    public static IServiceCollection AddAccountsInfrastructure(
        this IServiceCollection services,
        IConfiguration configuration)
    {
        services.AddDbContext<AccountsDbContext>(options =>
            options.UseSqlServer(configuration.GetConnectionString("AccountsConnection")));

        services.AddScoped<UserRepository>();
        services.AddScoped<IUserDomainRepository>(sp => sp.GetRequiredService<UserRepository>());
        services.AddScoped<IUserQueryRepository>(sp => sp.GetRequiredService<UserRepository>());

        return services
            .AddSingleton<IPasswordHasher, PasswordHasher>()
            .AddSingleton<ITokenGenerator, TokenGenerator>();
    }
}
