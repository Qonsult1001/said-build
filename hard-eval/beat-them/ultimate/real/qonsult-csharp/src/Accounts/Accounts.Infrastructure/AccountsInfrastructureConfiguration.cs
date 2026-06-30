// Accounts.Infrastructure — DI wiring for persistence + security services.

using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

public static class AccountsInfrastructureConfiguration
{
    public static IServiceCollection AddAccountsInfrastructure(
        this IServiceCollection services,
        IConfiguration configuration)
    {
        services.AddDbContext<AccountsDbContext>(options =>
            options.UseSqlServer(configuration.GetConnectionString("Accounts")));

        var jwtSettings = configuration.GetSection("Jwt").Get<JwtSettings>() ?? new JwtSettings();
        services.AddSingleton(jwtSettings);

        services.AddScoped<IAccountDomainRepository, AccountRepository>();
        services.AddScoped<IAccountQueryRepository, AccountRepository>();
        services.AddScoped<IAccountFactory, AccountFactory>();
        services.AddScoped<ITokenIssuer, JwtTokenIssuer>();
        services.AddSingleton<IPasswordHasher, Pbkdf2PasswordHasher>();

        return services;
    }
}
