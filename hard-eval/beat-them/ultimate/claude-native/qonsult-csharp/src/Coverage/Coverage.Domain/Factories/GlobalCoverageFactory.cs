// Internal fluent builder for GlobalCoverage. Build() asserts required fields then news the aggregate.
internal class GlobalCoverageFactory : IGlobalCoverageFactory
{
    private Guid businessId;
    private string? countryName;
    private string countryCode = string.Empty;
    private string? currency;

    public IGlobalCoverageFactory WithBusinessId(Guid businessId)
    {
        this.businessId = businessId;
        return this;
    }

    public IGlobalCoverageFactory WithCountry(string countryName, string countryCode)
    {
        this.countryName = countryName;
        this.countryCode = countryCode;
        return this;
    }

    public IGlobalCoverageFactory WithCurrency(string currency)
    {
        this.currency = currency;
        return this;
    }

    public GlobalCoverage Build()
    {
        if (string.IsNullOrWhiteSpace(countryName))
        {
            throw new InvalidOperationException("Cannot build a GlobalCoverage without a country name.");
        }

        if (string.IsNullOrWhiteSpace(currency))
        {
            throw new InvalidOperationException("Cannot build a GlobalCoverage without a currency.");
        }

        return new GlobalCoverage(businessId, countryName, countryCode, currency);
    }
}
