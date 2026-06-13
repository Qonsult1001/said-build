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
