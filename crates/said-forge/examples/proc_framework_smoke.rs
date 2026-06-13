//! Smoke test for the proc_framework module — bypasses said-cli plumbing.

use std::path::PathBuf;

fn main() -> Result<(), String> {
    let framework = PathBuf::from("dtcard/.forge/proc-framework");
    let profile = "webapi-sqlserver-cardissuing";

    println!("step 1: load Account bundle");
    let bundle = said_forge::proc_framework::manifest::load_bundle(&framework, "Account")?;
    println!("  ok — {} endpoints", bundle.endpoints.len());

    println!("step 2: filter to Account.CreateAccount");
    let row = bundle.endpoints
        .iter()
        .find(|e| e.id == "Account.CreateAccount")
        .ok_or("Account.CreateAccount not found")?;
    println!("  ok — verb={} entity={}", row.verb, row.entity);

    println!("step 3: load shape '{}'", row.shape);
    let shape = said_forge::proc_framework::shape::load_shape(&framework, profile, &row.shape)?;
    println!("  ok — {} regions", shape.regions.len());

    println!("step 4: render the proc");
    let rendered = said_forge::proc_framework::render::render_endpoint(&framework, profile, row, None)?;
    println!("  ok — {} bytes rendered", rendered.len());

    println!("step 5: parse the rendered text back through markers");
    let regions = said_forge::proc_framework::markers::parse_regions(&rendered)?;
    println!("  ok — {} regions detected in rendered output", regions.len());

    Ok(())
}
