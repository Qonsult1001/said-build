// Operation result used as the return type of every Application service. Web maps it to an HTTP
// response via ToActionResult(); services never throw for expected failures.
public class Result
{
    private readonly List<string> errors;

    internal Result(bool succeeded, List<string> errors)
    {
        Succeeded = succeeded;
        this.errors = errors;
    }

    public bool Succeeded { get; }

    public IReadOnlyList<string> Errors => errors.AsReadOnly();

    public static Result Success => new(true, new List<string>());

    public static Result Failure(IEnumerable<string> errors) => new(false, errors.ToList());

    public static Result Failure(params string[] errors) => new(false, errors.ToList());

    public static implicit operator Result(string error) => Failure(error);

    public static implicit operator Result(List<string> errors) => Failure(errors);
}
