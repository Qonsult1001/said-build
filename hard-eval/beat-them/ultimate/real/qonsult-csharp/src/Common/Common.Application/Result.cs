// Common.Application — the Result type services return; Web maps it via ToActionResult().

public class Result
{
    private readonly List<string> _errors;

    protected Result(bool succeeded, IEnumerable<string> errors)
    {
        _errors = errors.ToList();
        Succeeded = succeeded;
    }

    public bool Succeeded { get; }

    public IReadOnlyList<string> Errors => _errors.AsReadOnly();

    public static Result Success() => new(true, Array.Empty<string>());

    public static Result Failure(params string[] errors) => new(false, errors);

    public static Result<TData> Success<TData>(TData data) => new(data, true, Array.Empty<string>());

    public static Result<TData> Failure<TData>(params string[] errors) => new(default!, false, errors);
}

public class Result<TData> : Result
{
    internal Result(TData data, bool succeeded, IEnumerable<string> errors)
        : base(succeeded, errors) => Data = data;

    public TData Data { get; }
}
