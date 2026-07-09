// Fluent factory contract for the GlobalCoverage aggregate.
public interface IGlobalCoverageFactory
{
    IGlobalCoverageFactory WithBusinessId(Guid businessId);

    IGlobalCoverageFactory WithCountry(string countryName, string countryCode);

    IGlobalCoverageFactory WithCurrency(string currency);

    GlobalCoverage Build();
}
