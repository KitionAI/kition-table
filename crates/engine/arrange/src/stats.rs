//! How long arranging takes, for the editor's `gridStats` (performance tests read it).

use serde_json::{Value, json};

/// A stopwatch: `std::time::Instant` natively, `performance.now()` in the page.
pub struct Stopwatch {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    start: std::time::Instant,
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    start: f64,
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn performance_now() -> f64 {
    use js_sys::{Function, Reflect};
    let performance = Reflect::get(&js_sys::global(), &"performance".into()).ok();
    let now = performance.as_ref().and_then(|performance| Reflect::get(performance, &"now".into()).ok());
    match (performance, now.filter(|now| now.is_function()).map(Function::from)) {
        (Some(performance), Some(now)) => now.call0(&performance).ok().and_then(|value| value.as_f64()).unwrap_or_else(js_sys::Date::now),
        _ => js_sys::Date::now(),
    }
}

impl Stopwatch {
    #[cfg_attr(
        not(all(target_arch = "wasm32", target_os = "unknown")),
        allow(clippy::disallowed_methods, reason = "off wasm only; the page reads performance.now()")
    )]
    pub fn start() -> Self {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        return Self { start: std::time::Instant::now() };
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        return Self { start: performance_now() };
    }

    /// Milliseconds since [`Stopwatch::start`].
    pub fn elapsed_ms(&self) -> f64 {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        return self.start.elapsed().as_secs_f64() * 1000.0;
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        return performance_now() - self.start;
    }
}

/// The timings of one kind of work.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timing {
    pub count: u64,
    pub total_ms: f64,
    pub last_ms: f64,
    pub max_ms: f64,
    /// The records the last one arranged (shown and left out).
    pub last_records: u64,
}

impl Timing {
    pub fn record(&mut self, ms: f64, records: usize) {
        self.count += 1;
        self.total_ms += ms;
        self.last_ms = ms;
        self.max_ms = self.max_ms.max(ms);
        self.last_records = u64::try_from(records).unwrap_or(u64::MAX);
    }

    fn json(&self) -> Value {
        json!({ "count": self.count, "totalMs": self.total_ms, "lastMs": self.last_ms, "maxMs": self.max_ms, "lastRecords": self.last_records })
    }
}

/// The arrangement's timings: whole arrangements (`gridOpen`, and a `gridSync` that had to start
/// over) and relocations (a `gridSync` that moved only the records an edit changed).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub arrange: Timing,
    pub relocate: Timing,
    /// Bytes of the last axis answered.
    pub axis_bytes: u64,
}

impl Stats {
    pub fn json(&self) -> Value {
        json!({ "arrange": self.arrange.json(), "relocate": self.relocate.json(), "axisBytes": self.axis_bytes })
    }
}
