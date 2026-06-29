namespace Bench.CliCommand;

// Add User command (dotnet).  Sections: [Sn]..[/Sn] in run order.  Map + how to edit: ../said.index.md
// Tag on each section = can you edit it?   GENERATED (no, .said rewrites it from the blueprint) | YOURS (yes, kept).
// SAME command-shape skeleton as RemoveUserCommand (GENERATED 80% identical) -> harvest learns ONE `run`
// blueprint; only the YOURS 20% (execute) differs.
public class AddUserCommand
{
    private readonly IUserService _users;
    public AddUserCommand(IUserService users) { _users = users; }

    public async Task<int> Run(string[] args)
    {
        // [S1] parse-args  GENERATED
        var parsed = ArgParser.Parse(args);
        // [/S1]

        // [S2] validate  YOURS
        if (!parsed.Has("name")) { Console.Error.WriteLine("--name is required"); return 2; }
        if (!parsed.Has("email")) { Console.Error.WriteLine("--email is required"); return 2; }
        // [/S2]

        // [S3] load-context  GENERATED
        var ctx = await AppContext.Load();
        // [/S3]

        // [S4] execute  YOURS
        var id = await _users.Add(ctx, parsed.Get("name"), parsed.Get("email"));
        // [/S4]

        // [S5] print-result  GENERATED
        Console.WriteLine($"added user {id}");
        // [/S5]

        // [S6] return-exit-code  GENERATED
        return 0;
        // [/S6]
    }
}

public interface IUserService { Task<string> Add(object ctx, string name, string email); Task Remove(object ctx, string id); }
