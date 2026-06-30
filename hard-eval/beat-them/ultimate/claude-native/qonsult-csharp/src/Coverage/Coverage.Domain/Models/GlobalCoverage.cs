// Coverage context — GlobalCoverage aggregate root. One row per (business, country) the frontend
// matches a searched address against to resolve currency. Private setters; invariants in-aggregate.
public class GlobalCoverage : Entity, IAggregateRoot
{
    private GlobalCoverage()
    {
    }

    public GlobalCoverage(Guid businessId, string countryName, string countryCode, string currency)
    {
        ValidateCountryName(countryName);
        ValidateCurrency(currency);

        BusinessId = businessId;
        CountryName = countryName;
        CountryCode = countryCode ?? string.Empty;
        Currency = currency;
    }

    public Guid BusinessId { get; private set; }

    public string CountryName { get; private set; } = string.Empty;

    public string CountryCode { get; private set; } = string.Empty;

    public string Currency { get; private set; } = string.Empty;

    public bool IsActive { get; private set; } = true;

    public GlobalCoverage Deactivate()
    {
        IsActive = false;
        return this;
    }

    private static void ValidateCountryName(string countryName)
    {
        if (string.IsNullOrWhiteSpace(countryName))
        {
            throw new InvalidOperationException("Country name is required.");
        }

        if (countryName.Length > CoverageModelConstants.CountryNameMaxLength)
        {
            throw new InvalidOperationException(
                $"Country name must be at most {CoverageModelConstants.CountryNameMaxLength} characters.");
        }
    }

    private static void ValidateCurrency(string currency)
    {
        if (string.IsNullOrWhiteSpace(currency))
        {
            throw new InvalidOperationException("Currency is required.");
        }
    }
}
