//! Cross-target time helper.
//!
//! Native uses `std::time::SystemTime`; wasm32 uses `js_sys::Date::now()`
//! (the browser's clock). `js-sys` is a target-conditional dependency so
//! it doesn't pollute the native build.

/// Unix epoch seconds. Returns 0 if the clock is not available.
pub fn unix_secs() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        // js_sys::Date::now() returns f64 milliseconds since epoch.
        (js_sys::Date::now() / 1000.0) as u64
    }
}

/// Today's date as `(year, month, day)` from the system clock. Callers at the I/O boundary
/// (CLI / MCP handler) use this to feed `ground_relative_dates`; `sca-core`'s pure transform
/// never reads the clock itself, so stored bytes stay reproducible and the transform is testable.
pub fn today_ymd() -> (i32, u32, u32) {
    let secs = unix_secs() as i64;
    civil_from_days(secs.div_euclid(86_400))
}

/// Convert a count of days since the Unix epoch to a civil `(year, month, day)`.
/// Howard Hinnant's `civil_from_days` algorithm — exact, no external date crate, no leap-year
/// special-casing bugs. Used only to derive "today" for relative-date grounding.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    ((y + if m <= 2 { 1 } else { 0 }) as i32, m as u32, d as u32)
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December"];

/// WRITE-TIME temporal grounding (research-proven Mem0 Layer-1, done deterministically).
///
/// Resolve relative time phrases in a memory against `today` (`year`/`month`/`day`, passed in
/// so the transform is pure/reproducible — never the wall clock) and APPEND the absolute date so
/// later semantic recall finds the memory. Original text is preserved; grounding is additive and
/// idempotent (re-grounding an already-grounded string is a no-op). No LLM.
///
/// Handled phrases (case-insensitive, whole-phrase): "last year"→prior year, "this year"→current
/// year, "last quarter"→prior calendar quarter (wraps year), "last month"→prior month (wraps year).
/// Anything else is left untouched — the goal is high-precision common cases, not full NLP.
pub fn ground_relative_dates(text: &str, year: i32, month: u32, day: u32) -> String {
    let _ = day; // day granularity not needed for the year/quarter/month phrases handled here
    let lower = text.to_lowercase();
    let mut suffixes: Vec<String> = Vec::new();

    let mut add = |phrase_present: bool, token: String| {
        // Idempotency + no-double-ground: only append if the resolved absolute token isn't already
        // somewhere in the text (an already-grounded or already-absolute memory stays untouched).
        if phrase_present && !lower.contains(&token.to_lowercase())
            && !suffixes.iter().any(|s| s.contains(&token)) {
            suffixes.push(token);
        }
    };

    // "last year" → prior year; "this year" / "earlier this year" → current year.
    add(lower.contains("last year"), (year - 1).to_string());
    add(lower.contains("this year"), year.to_string());

    // "last quarter" → prior calendar quarter (wraps to Q4 of prior year from Q1).
    if lower.contains("last quarter") {
        let cur_q = ((month - 1) / 3) + 1; // 1..=4
        let (lq, lqy) = if cur_q == 1 { (4, year - 1) } else { (cur_q - 1, year) };
        add(true, format!("Q{lq} {lqy}"));
    }

    // "last month" → prior calendar month (wraps to December of prior year from January).
    if lower.contains("last month") {
        let (lm, lmy) = if month == 1 { (12u32, year - 1) } else { (month - 1, year) };
        add(true, format!("{} {}", MONTHS[(lm - 1) as usize], lmy));
    }

    if suffixes.is_empty() {
        return text.to_string();
    }
    // Append as a single unobtrusive parenthetical the encoder + lexical index will both see.
    format!("{} (around {})", text.trim_end(), suffixes.join(", "))
}

/// A cross-target stopwatch for profiling. On native it wraps
/// `std::time::Instant`; on wasm32 it is a no-op that always reports 0
/// elapsed (profiling metrics aren't meaningful without a monotonic clock,
/// and `Instant::now()` panics on wasm32-unknown-unknown).
pub struct Stopwatch {
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
}

impl Stopwatch {
    pub fn start() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            start: std::time::Instant::now(),
        }
    }
    /// Elapsed microseconds since `start()`. 0 on wasm32.
    pub fn elapsed_micros(&self) -> u64 {
        #[cfg(not(target_arch = "wasm32"))]
        { self.start.elapsed().as_micros() as u64 }
        #[cfg(target_arch = "wasm32")]
        { 0 }
    }
    /// Elapsed milliseconds (f64) since `start()`. 0.0 on wasm32.
    pub fn elapsed_ms(&self) -> f64 {
        #[cfg(not(target_arch = "wasm32"))]
        { self.start.elapsed().as_secs_f64() * 1000.0 }
        #[cfg(target_arch = "wasm32")]
        { 0.0 }
    }
}
