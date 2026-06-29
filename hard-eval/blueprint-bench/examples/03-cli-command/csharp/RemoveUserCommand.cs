namespace Bench.CliCommand;

// SAME command shape as AddUserCommand (80% skeleton identical); only the execute slot differs.
// Two occurrences => harvest learns ONE "command" blueprint.
public class RemoveUserCommand
{
    private readonly IUserService _users;
    public RemoveUserCommand(IUserService users) { _users = users; }

    public async Task<int> Run(string[] args)
    {
        // [80%] parse args
        var parsed = ArgParser.Parse(args);
        // [80%] validate
        if (!parsed.Has("id")) { Console.Error.WriteLine("--id is required"); return 2; }
        // [80%] load context
        var ctx = await AppContext.Load();
        // [20%] execute (command-specific)
        await _users.Remove(ctx, parsed.Get("id"));
        // [80%] print result
        Console.WriteLine($"removed user {parsed.Get("id")}");
        // [80%] return exit code
        return 0;
    }
}
