//! Iteration-note compression — Claude Code's two mechanisms, faithfully:
//!   1. per-section cap (Claude: ~2000 tokens/section)
//!   2. total cap with cycle-out (Claude: ~12000 total, "condense by cycling out
//!      less important details while preserving the most critical")
//!
//! "Preserve the most critical" = keep the HEAD of each section (the decision /
//! summary an author leads with) and drop the trailing detail when over budget.
//! Section order is the cycle-out priority: later sections (Worklog, Codebase
//! docs) yield first; earlier ones (Title, Current State, Task, Errors) are
//! protected. Limits are tunable constants — we set sane defaults and refine
//! against real notes.
//!
//! Token budgeting uses a chars≈4/token estimate (same rough basis Claude uses
//! in roughTokenCountEstimation); we work in chars to stay dependency-free.

/// ~2000 tokens/section ≈ 8000 chars.
pub const MAX_SECTION_CHARS: usize = 8_000;
/// ~12000 tokens total ≈ 48000 chars.
pub const MAX_TOTAL_CHARS: usize = 48_000;

/// Cycle-out priority: sections are dropped/trimmed from the BOTTOM of this list
/// first. The protected, high-value sections are at the top.
const SECTION_PRIORITY: &[&str] = &[
    "Title",
    "Current State",
    "Task",
    "Errors and Corrections",
    "Key Results",
    "Files and Functions",
    "Learnings",
    "Workflow",
    "Codebase and System Documentation",
    "Worklog",
];

struct Section {
    header: String, // full "# Header" line
    name: String,   // "Header"
    body: String,   // everything under the header (incl. the italic _desc_)
}

fn parse_sections(note: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut cur: Option<Section> = None;
    for line in note.lines() {
        if let Some(name) = line.strip_prefix("# ") {
            if let Some(s) = cur.take() {
                out.push(s);
            }
            cur = Some(Section {
                header: line.to_string(),
                name: name.trim().to_string(),
                body: String::new(),
            });
        } else if let Some(s) = cur.as_mut() {
            s.body.push_str(line);
            s.body.push('\n');
        }
    }
    if let Some(s) = cur.take() {
        out.push(s);
    }
    out
}

/// Keep the head of `s` up to `max` chars on a line boundary (don't cut a line
/// mid-way); append a marker when trimmed.
fn cap_head(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut kept = String::new();
    for line in s.lines() {
        if kept.len() + line.len() + 1 > max {
            break;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    kept.push_str("_(condensed — older detail cycled out)_\n");
    kept
}

/// Compress an iteration note: per-section cap, then total cap by cycling out the
/// lowest-priority sections. Returns the compressed note.
pub fn compress_note(note: &str) -> String {
    let mut sections = parse_sections(note);
    if sections.is_empty() {
        // Not in template form — just cap the whole thing.
        return cap_head(note, MAX_TOTAL_CHARS);
    }

    // 1. Per-section cap.
    for s in &mut sections {
        s.body = cap_head(&s.body, MAX_SECTION_CHARS);
    }

    // 2. Total cap with cycle-out: while over budget, trim then drop the
    //    lowest-priority non-empty section.
    let render = |secs: &[Section]| -> String {
        secs.iter()
            .map(|s| format!("{}\n{}", s.header, s.body.trim_end()))
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    if render(&sections).len() <= MAX_TOTAL_CHARS {
        return render(&sections);
    }

    // priority index: lower = more protected. Unknown sections rank lowest.
    let prio = |name: &str| {
        SECTION_PRIORITY
            .iter()
            .position(|p| p.eq_ignore_ascii_case(name))
            .unwrap_or(usize::MAX)
    };

    // Drop from the lowest-priority end until under budget (never drop the top 4).
    loop {
        let total = render(&sections).len();
        if total <= MAX_TOTAL_CHARS || sections.len() <= 4 {
            break;
        }
        // Find the lowest-priority section to drop.
        let mut victim = 0usize;
        let mut worst = 0usize;
        for (i, s) in sections.iter().enumerate() {
            let p = prio(&s.name);
            if p >= worst {
                worst = p;
                victim = i;
            }
        }
        sections.remove(victim);
    }

    let mut rendered = render(&sections);
    if rendered.len() > MAX_TOTAL_CHARS {
        rendered = cap_head(&rendered, MAX_TOTAL_CHARS);
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_note_unchanged_in_substance() {
        let note = "# Title\nFix endpoint\n\n# Task\nadd /api/cores\n";
        let out = compress_note(note);
        assert!(out.contains("Fix endpoint"));
        assert!(out.contains("add /api/cores"));
    }

    #[test]
    fn over_budget_section_is_capped() {
        let big = "x\n".repeat(MAX_SECTION_CHARS); // ~2x over a section
        let note = format!("# Title\nt\n\n# Worklog\n{}", big);
        let out = compress_note(&note);
        assert!(out.len() < note.len());
        assert!(out.contains("condensed"));
    }

    #[test]
    fn protects_top_sections_drops_low_priority() {
        let big = "y\n".repeat(MAX_TOTAL_CHARS); // force total cap
        let note = format!(
            "# Title\nt\n\n# Current State\ndone\n\n# Task\nthe task\n\n# Errors and Corrections\navoid X\n\n# Worklog\n{}",
            big
        );
        let out = compress_note(&note);
        // High-value sections survive; Worklog (lowest priority) is trimmed/dropped.
        assert!(out.contains("# Title"));
        assert!(out.contains("# Task"));
        assert!(out.contains("avoid X"));
    }
}
