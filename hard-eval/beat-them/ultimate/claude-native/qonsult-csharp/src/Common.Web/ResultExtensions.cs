using Microsoft.AspNetCore.Mvc;

// Maps an Application Result/Result<T> onto an ActionResult. The single place that knows how a domain
// outcome becomes an HTTP status, so controllers stay one-liners.
public static class ResultExtensions
{
    public static async Task<ActionResult> ToActionResult(this Task<Result> resultTask)
        => Map(await resultTask);

    public static async Task<ActionResult> ToActionResult<TData>(this Task<Result<TData>> resultTask)
        => Map(await resultTask);

    private static ActionResult Map(Result result)
        => result.Succeeded
            ? new OkResult()
            : new BadRequestObjectResult(new { errors = result.Errors });

    private static ActionResult Map<TData>(Result<TData> result)
        => result.Succeeded
            ? new OkObjectResult(result.Data)
            : new BadRequestObjectResult(new { errors = result.Errors });
}
