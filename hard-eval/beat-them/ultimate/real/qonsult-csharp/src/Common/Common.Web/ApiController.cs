// Common.Web — base controller. Inheriting gives the api/[controller]/[action] route shape.

using Microsoft.AspNetCore.Mvc;

[ApiController]
[Route("api/[controller]/[action]")]
public abstract class ApiController : ControllerBase
{
}

public static class ResultExtensions
{
    public static async Task<ActionResult> ToActionResult(this Task<Result> resultTask)
    {
        var result = await resultTask;
        return result.Succeeded
            ? new OkResult()
            : new BadRequestObjectResult(new { errors = result.Errors });
    }

    public static async Task<ActionResult> ToActionResult<TData>(this Task<Result<TData>> resultTask)
    {
        var result = await resultTask;
        return result.Succeeded
            ? new OkObjectResult(result.Data)
            : new BadRequestObjectResult(new { errors = result.Errors });
    }
}
