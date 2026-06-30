// Common.Application — registers FluentValidation validators for the calling context's assembly.

using System.Reflection;
using FluentValidation;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

public static class CommonApplicationConfiguration
{
    public static IServiceCollection AddCommonApplication(
        this IServiceCollection services,
        IConfiguration configuration,
        Assembly applicationAssembly)
    {
        services.AddValidatorsFromAssembly(applicationAssembly);
        return services;
    }
}
