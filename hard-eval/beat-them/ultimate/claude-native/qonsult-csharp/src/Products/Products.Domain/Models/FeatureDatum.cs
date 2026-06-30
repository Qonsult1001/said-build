// Products context — FeatureDatum aggregate root. A named product feature the frontend looks up by
// `name` (featureData.find(item => item.name === feature)). GET /products/featuredata/ lists them.
public class FeatureDatum : Entity, IAggregateRoot
{
    private FeatureDatum()
    {
    }

    public FeatureDatum(string name, string? description, bool isActive)
    {
        ValidateName(name);

        Name = name;
        Description = description;
        IsActive = isActive;
    }

    public string Name { get; private set; } = string.Empty;

    public string? Description { get; private set; }

    public bool IsActive { get; private set; } = true;

    private static void ValidateName(string name)
    {
        if (string.IsNullOrWhiteSpace(name))
        {
            throw new InvalidOperationException("A feature name is required.");
        }

        if (name.Length > ProductModelConstants.FeatureNameMaxLength)
        {
            throw new InvalidOperationException(
                $"Feature name must be at most {ProductModelConstants.FeatureNameMaxLength} characters.");
        }
    }
}
