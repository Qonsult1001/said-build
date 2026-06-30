using Microsoft.Extensions.DependencyInjection;

// Web-layer registration hook for the Accounts context (controllers are discovered via the host's
// AddControllers; this is the per-context seam the composition root calls).
public static class AccountsWebConfiguration
{
    public static IServiceCollection AddAccountsWebComponents(this IServiceCollection services)
        => services;
}
