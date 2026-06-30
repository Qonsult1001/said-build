// Result carrying a payload. Command/query services return Result<TResponse>; the payload is always a
// response DTO, never a domain entity.
public class Result<TData> : Result
{
    private readonly TData? data;

    internal Result(bool succeeded, TData? data, List<string> errors)
        : base(succeeded, errors)
    {
        this.data = data;
    }

    public TData Data => Succeeded
        ? data!
        : throw new InvalidOperationException("Cannot access data of a failed result.");

    public static Result<TData> SuccessWith(TData data) => new(true, data, new List<string>());

    public static new Result<TData> Failure(IEnumerable<string> errors) => new(false, default, errors.ToList());

    public static new Result<TData> Failure(params string[] errors) => new(false, default, errors.ToList());

    public static implicit operator Result<TData>(TData data) => SuccessWith(data);

    public static implicit operator Result<TData>(string error) => Failure(error);

    public static implicit operator Result<TData>(List<string> errors) => Failure(errors);
}
