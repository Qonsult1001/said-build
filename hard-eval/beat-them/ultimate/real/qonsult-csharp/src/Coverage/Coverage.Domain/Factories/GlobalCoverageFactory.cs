// Coverage.Domain — fluent factory for GlobalCoverage.

public interface IGlobalCoverageFactory
{
    IGlobalCoverageFactory WithUserId(Guid userId);
    GlobalCoverage Build();
}

internal class GlobalCoverageFactory : IGlobalCoverageFactory
{
    private Guid? _userId;

    public IGlobalCoverageFactory WithUserId(Guid userId)
    {
        _userId = userId;
        return this;
    }

    public GlobalCoverage Build()
    {
        if (_userId is null)
        {
            throw new InvalidOperationException("UserId is required to build GlobalCoverage.");
        }

        return new GlobalCoverage(_userId.Value);
    }
}
