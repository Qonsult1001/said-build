// Add User command (rust).  Sections: [Sn]..[/Sn] in run order.  Map + how to edit: said.index.md
// Tag on each section = can you edit it?   GENERATED (no, .said rewrites it from the blueprint) | YOURS (yes, kept).
// Rendered cross-language from the recalled `run` (command) blueprint harvested from C#; the GENERATED
// sections ARE the reused 80% skeleton, the YOURS section is the command-specific 20% (execute).
pub async fn run(args: &[String]) -> i32 {
    // [S1] parse-args  GENERATED
    let parsed = ArgParser::parse(args);
    // [/S1]

    // [S2] validate  YOURS
    if !parsed.has("name") { eprintln!("--name is required"); return 2; }
    if !parsed.has("email") { eprintln!("--email is required"); return 2; }
    // [/S2]

    // [S3] load-context  GENERATED
    let ctx = AppContext::load().await;
    // [/S3]

    // [S4] execute  YOURS
    let id = users_add(&ctx, parsed.get("name"), parsed.get("email")).await;
    // [/S4]

    // [S5] print-result  GENERATED
    println!("added user {id}");
    // [/S5]

    // [S6] return-exit-code  GENERATED
    0
    // [/S6]
}
