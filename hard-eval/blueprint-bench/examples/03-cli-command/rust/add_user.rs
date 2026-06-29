// RENDERED from the recalled `run<Entity>` (command) blueprint (harvested from AddUserCommand/RemoveUserCommand).
// The [80%] skeleton (parse args -> validate -> load context -> ... -> print result -> return exit code)
// came from .said's recalled blueprint, rendered in rust. Only the [20%] execute slot was written.
pub async fn run(args: &[String]) -> i32 {
    // [80%] parse args               (blueprint: Parse)
    let parsed = ArgParser::parse(args);
    // [80%] validate                 (blueprint: Has, WriteLine)
    if !parsed.has("name") { eprintln!("--name is required"); return 2; }
    if !parsed.has("email") { eprintln!("--email is required"); return 2; }
    // [80%] load context             (blueprint: Load)
    let ctx = AppContext::load().await;
    // [20%] execute (command-specific)
    let id = users_add(&ctx, parsed.get("name"), parsed.get("email")).await;
    // [80%] print result             (blueprint: WriteLine)
    println!("added user {id}");
    // [80%] return exit code
    0
}
