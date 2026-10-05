//! How late the picture is: the numbers that keep UwUCast honest.
//!
//! A sender stamps every frame with the moment the screen showed it, as
//! wall-clock microseconds since 1970 (see [`crate::protocol`]). The
//! receiver subtracts that from its own clock when the frame is in: capture
//! to received, everything the sending computer and the network add. On one
//! computer that is exact; between two, it also holds whatever the two clocks
//! disagree by (Windows keeps them within a few milliseconds of each other
//! when both sync with the internet, but not always), so a value far off is
//! reported as a clock difference, not as latency.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Further apart than this, sender and receiver clocks just differ.
pub const PLAUSIBLE: Duration = Duration::from_secs(5);

/// Now, as microseconds since 1970.
pub fn wall_clock_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_micros() as u64)
}

/// How long ago `stamp_us` (a sender's wall-clock stamp) was, if that is a
/// believable latency.
pub fn age_us(stamp_us: u64) -> Option<i64> {
    let age = wall_clock_us() as i64 - stamp_us as i64;
    (age.unsigned_abs() < PLAUSIBLE.as_micros() as u64).then_some(age)
}

/// Latencies collected over a while, summed up as median, 95th percentile
/// and worst.
#[derive(Debug, Default, Clone)]
pub struct Stats {
    samples: Vec<i64>,
    /// Frames whose stamp was too far off to count.
    pub implausible: usize,
    /// Bytes of video, for the bit rate.
    pub bytes: usize,
}

/// One summary of [`Stats`], in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub count: usize,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "median {:.1} ms, p95 {:.1} ms, max {:.1} ms over {} frames",
            self.median_ms, self.p95_ms, self.max_ms, self.count
        )
    }
}

impl Stats {
    /// Counts a frame stamped `stamp_us` that is `bytes` long, arriving now.
    pub fn frame(&mut self, stamp_us: u64, bytes: usize) {
        self.bytes += bytes;
        match age_us(stamp_us) {
            Some(age) => self.samples.push(age),
            None => self.implausible += 1,
        }
    }

    /// Counts a latency measured elsewhere, in microseconds.
    pub fn sample(&mut self, age_us: i64) {
        self.samples.push(age_us);
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn summary(&self) -> Option<Summary> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize] as f64 / 1000.0;
        Some(Summary {
            count: sorted.len(),
            median_ms: at(0.5),
            p95_ms: at(0.95),
            max_ms: at(1.0),
        })
    }

    /// Starts over, e.g. after a summary was logged.
    pub fn clear(&mut self) {
        self.samples.clear();
        self.implausible = 0;
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_of_known_values() {
        let mut stats = Stats::default();
        for ms in 1..=100 {
            stats.sample(ms * 1000);
        }
        let summary = stats.summary().unwrap();
        assert_eq!(summary.count, 100);
        assert_eq!(summary.median_ms, 51.0);
        assert_eq!(summary.p95_ms, 95.0);
        assert_eq!(summary.max_ms, 100.0);
    }

    #[test]
    fn stamps_from_another_clock_are_not_latency() {
        let mut stats = Stats::default();
        stats.frame(wall_clock_us() - 20_000, 100);
        // An old sender's stamp: microseconds since it started.
        stats.frame(33_333, 100);
        assert_eq!((stats.len(), stats.implausible, stats.bytes), (1, 1, 200));
        let summary = stats.summary().unwrap();
        assert!((19.0..1000.0).contains(&summary.median_ms), "{summary}");
    }
}
