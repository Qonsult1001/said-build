namespace Bench.CliCommand;

// CLI command-handler example, command = AddUser. The "command" shape: parse args -> validate ->
// load context -> execute -> print result -> return exit code. The SAME shape appears in
// RemoveUserCommand (support>=2) so harvest learns ONE "command" blueprint; only the execute slot differs.
public class AddUserCommand
{
    private readonly IUserService _users;
    public AddUserCommand(IUserService users) { _users = users; }

    public async Task<int> Run(string[] args)
    {
        // [80%] parse args
        var parsed = ArgParser.Parse(args);
        // [80%] validate
        if (!parsed.Has("name")) { Console.Error.WriteLine("--name is required"); return 2; }
        if (!parsed.Has("email")) { Console.Error.WriteLine("--email is required"); return 2; }
        // [80%] load context
        var ctx = await AppContext.Load();
        // [20%] execute (command-specific)
        var id = await _users.Add(ctx, parsed.Get("name"), parsed.Get("email"));
        // [80%] print result
        Console.WriteLine($"added user {id}");
        // [80%] return exit code
        return 0;
    }
}

public interface IUserService { Task<string> Add(object ctx, string name, string email); Task Remove(object ctx, string id); }
