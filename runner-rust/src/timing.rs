// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Timing that survives a shared cloud runner.
//!
//! The machine this runs on is not quiet. A neighbouring tenant, a frequency
//! change or a scheduler decision can move a measurement by tens of percent,
//! and none of that is a property of an encoding. Three things are done about
//! it, in order of how much they matter:
//!
//! 1. **Everything is reported relative to Base64 from the same round.** This
//!    is the whole game. If the machine is 20 % slow for a stretch, it is 20 %
//!    slow for Base64 too, and the ratio does not move. Absolute MB/s is
//!    recorded as well, but it is the number that should be distrusted.
//! 2. **Codecs are interleaved within a round**, not run one after another to
//!    completion. A slow stretch that lands entirely inside one codec's block
//!    would look exactly like that codec being slow; a slow stretch spread
//!    across a round hits every codec in it.
//! 3. **The median across rounds is reported, with the interquartile range
//!    beside it.** A single stalled round moves a median by nothing and an
//!    average by a lot -- and the IQR is what says whether a difference between
//!    two codecs means anything at all.
//!
//! Short inputs get one more thing: an operation that takes less time than the
//! clock can resolve is repeated until the window is long enough to measure,
//! and the count is divided out. Without it every sample under a few hundred
//! bytes would report the same number, which is the clock's number and not the
//! encoding's.

use std::time::Instant;

/// How long a single timing window must last before it is believed. Well above
/// the resolution of the clock and above the cost of reading it, and short
/// enough that a corpus of this size still finishes.
const MIN_WINDOW_NS: u128 = 300_000;

/// Runs `f` enough times to fill a window that can be measured, and returns the
/// nanoseconds one call took.
pub fn time_once(mut f: impl FnMut()) -> f64 {
    let mut reps: u32 = 1;
    loop {
        let start = Instant::now();
        for _ in 0..reps {
            f();
        }
        let elapsed = start.elapsed().as_nanos();
        if elapsed >= MIN_WINDOW_NS || reps >= 1 << 22 {
            return elapsed as f64 / reps as f64;
        }
        // Doubling rather than extrapolating: an extrapolation from a
        // measurement too short to trust is a guess dressed as arithmetic.
        reps = reps.saturating_mul(4);
    }
}

/// The measurements of one cell across rounds, reduced to what gets reported.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Summary {
    /// Median nanoseconds per operation.
    pub ns: f64,
    /// Interquartile range, in nanoseconds. The width of the middle half of the
    /// rounds: how much of the reported figure is the machine rather than the
    /// code. A difference between two codecs smaller than this is not a result.
    pub iqr_ns: f64,
    pub rounds: usize,
}

impl Summary {
    pub fn of(samples: &mut [f64]) -> Summary {
        assert!(!samples.is_empty(), "no rounds recorded");
        samples.sort_by(|a, b| a.partial_cmp(b).expect("no NaN from a clock"));
        Summary {
            ns: quantile(samples, 0.5),
            iqr_ns: quantile(samples, 0.75) - quantile(samples, 0.25),
            rounds: samples.len(),
        }
    }
}

/// Linear-interpolated quantile of an already sorted slice.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = q * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_ignores_one_stalled_round() {
        let mut clean = [10.0, 10.0, 10.0, 10.0, 10.0];
        let mut stalled = [10.0, 10.0, 10.0, 10.0, 1000.0];
        assert_eq!(Summary::of(&mut clean).ns, Summary::of(&mut stalled).ns);
    }

    #[test]
    fn iqr_reports_the_spread() {
        let mut steady = [10.0, 10.0, 10.0, 10.0, 10.0];
        let mut noisy = [1.0, 5.0, 10.0, 15.0, 20.0];
        assert_eq!(Summary::of(&mut steady).iqr_ns, 0.0);
        assert!(Summary::of(&mut noisy).iqr_ns > 0.0);
    }

    #[test]
    fn a_fast_operation_is_measured_rather_than_the_clock() {
        // A no-op must come out well under a microsecond; without repetition it
        // would come out as whatever one clock read costs.
        let ns = time_once(|| {
            std::hint::black_box(1u64 + 1);
        });
        assert!(ns < 1000.0, "{ns} ns for an addition");
    }
}
