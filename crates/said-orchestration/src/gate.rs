//! The build/test gate — the SOLE judge of correctness in the loop.
//!
//! Runs the project's build and test commands; green only if both succeed. The
//! orchestrator NEVER declares work done on the LLM's say-so — only on a green
//! gate. On red, the captured output feeds the repair phase. This is the rail
//! that stops the loop amplifying its own mistakes.

use std::process::Command;

/// The commands that constitute "does this change actually work".
#[derive(Debug, Clone)]
pub struct GateRunner {
    /// Working directory to run in (the project root).
    pub cwd: String,
    /// Build command + args, e.g. ["cargo", "build"] or ["dotnet", "build"].
    pub build: Vec<String>,
    /// Test command + args, e.g. ["cargo", "test"]. Empty = skip tests.
    pub test: Vec<String>,
}

/// Result of running the gate.
#[derive(Debug, Clone)]
pub struct GateOutcome {
    pub green: bool,
    /// Combined stdout+stderr of whichever step failed (or a short OK note).
    pub output: String,
    /// Which step failed, for the repair prompt ("build" | "test" | "").
    pub failed_step: String,
}

impl GateRunner {
    fn run_one(&self, cmd: &[String]) -> Result<(bool, String), String> {
        let (prog, args) = cmd.split_first().ok_or("empty gate command")?;
        let out = Command::new(prog)
            .args(args)
            .current_dir(&self.cwd)
            .output()
            .map_err(|e| format!("spawn `{}`: {}", prog, e))?;
        let mut combined = String::from_utf8_lossy(&out.stdout).to_string();
        combined.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok((out.status.success(), combined))
    }

    /// Run build then test. Green only if both pass. On failure, returns the
    /// failing step's output (capped) for the repair phase.
    pub fn run(&self) -> GateOutcome {
        if !self.build.is_empty() {
            match self.run_one(&self.build) {
                Ok((true, _)) => {}
                Ok((false, out)) => {
                    return GateOutcome { green: false, output: cap(&out), failed_step: "build".into() }
                }
                Err(e) => {
                    return GateOutcome { green: false, output: e, failed_step: "build".into() }
                }
            }
        }
        if !self.test.is_empty() {
            match self.run_one(&self.test) {
                Ok((true, _)) => {}
                Ok((false, out)) => {
                    return GateOutcome { green: false, output: cap(&out), failed_step: "test".into() }
                }
                Err(e) => {
                    return GateOutcome { green: false, output: e, failed_step: "test".into() }
                }
            }
        }
        GateOutcome { green: true, output: "build + test passed".into(), failed_step: String::new() }
    }
}

/// Cap gate output so a huge build log doesn't blow the repair prompt's context.
/// Keep the TAIL — compiler errors are usually at the end.
fn cap(s: &str) -> String {
    const MAX: usize = 6000;
    if s.len() <= MAX {
        s.to_string()
    } else {
        let tail = &s[s.len() - MAX..];
        format!("...(truncated, showing last {} chars)...\n{}", MAX, tail)
    }
}
