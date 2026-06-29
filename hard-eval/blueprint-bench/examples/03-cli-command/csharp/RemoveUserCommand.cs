namespace Bench.CliCommand;

// Remove User command (dotnet).  Sections: [Sn]..[/Sn] in run order.  Map + how to edit: ../said.index.md
// Tag on each section = can you edit it?   GENERATED (no, .said rewrites it from the blueprint) | YOURS (yes, kept).
// SAME command-shape skeleton as AddUserCommand (GENERATED 80% identical); only the YOURS 20% differs.
public class RemoveUserCommand
{
    private readonly IUserService _users;
    public RemoveUserCommand(IUserService users) { _users = users; }

    public async Task<int> Run(string[] args)
    {
        // [S1] parse-args  GENERATED
        var parsed = ArgParser.Parse(args);
        // [/S1]

        // [S2] validate  YOURS
        if (!parsed.Has("id")) { Console.Error.WriteLine("--id is required"); return 2; }
        // [/S2]

        // [S3] load-context  GENERATED
        var ctx = await AppContext.Load();
        // [/S3]

        // [S4] execute  YOURS
        await _users.Remove(ctx, parsed.Get("id"));
        // [/S4]

        // [S5] print-result  GENERATED
        Console.WriteLine($"removed user {parsed.Get("id")}");
        // [/S5]

        // [S6] return-exit-code  GENERATED
        return 0;
        // [/S6]
    }
}
