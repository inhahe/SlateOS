//! A fixed-capacity history of `f32` samples, for the graphs that plot one.
//!
//! Three widgets had grown their own copy of this — the resource monitor's
//! `GraphData`, and a `GraphHistory` each in the process explorer and the
//! system monitor. All three were the same ring buffer written out again: a
//! sample array, a write cursor wrapped with `% CAPACITY`, and a count that
//! stops climbing once the buffer is full. The copies had already drifted:
//! one advanced its cursor with `+ 1`, one with `wrapping_add(1)`, one indexed
//! its array directly while the others went through `get`; `peak` folded from
//! negative infinity in one and from zero in another, so the same samples gave
//! two different answers; and only one of the three could report an average.
//!
//! Written here once, the wrap goes through [`step`](crate::step), which carries the "the
//! buffer is not empty" condition inside the expression that depends on it, so
//! a capacity of zero is a history that holds nothing rather than a division by
//! zero or an index off the end.
//!
//! The capacity is a runtime argument rather than a const generic because the
//! three callers want three different lengths (120, 60 and 90 samples) chosen
//! to match the width of the graph they feed, and because a history is built
//! once per metric and then only pushed to — there is nothing here worth
//! monomorphising for.

use crate::step;

/// The last `capacity` samples of a value that is measured repeatedly.
///
/// Pushing past the capacity silently drops the oldest sample, which is what a
/// scrolling graph wants: the buffer holds exactly the window that is on
/// screen.
#[derive(Clone, Debug, PartialEq)]
pub struct SampleHistory {
    /// Storage, always exactly `capacity` long. Slots past `count` are zero.
    samples: Vec<f32>,
    /// The next slot to overwrite.
    cursor: usize,
    /// How many samples have been pushed, stopping at the capacity.
    count: usize,
}

impl SampleHistory {
    /// An empty history that will hold `capacity` samples.
    ///
    /// A capacity of zero is allowed and yields a history that never holds
    /// anything; every method below stays total on it rather than the caller
    /// having to prove the capacity is non-zero.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: vec![0.0; capacity],
            cursor: 0,
            count: 0,
        }
    }

    /// How many samples this history can hold.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.samples.len()
    }

    /// How many samples it currently holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether no sample has been pushed yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Whether the next push will drop the oldest sample.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.count >= self.capacity()
    }

    /// Forget every sample, keeping the capacity.
    pub fn clear(&mut self) {
        self.samples.fill(0.0);
        self.cursor = 0;
        self.count = 0;
    }

    /// Record a sample, dropping the oldest if the history is full.
    pub fn push(&mut self, value: f32) {
        if let Some(slot) = self.samples.get_mut(self.cursor) {
            *slot = value;
        }
        self.cursor = step::wrapping_after(self.capacity(), self.cursor);
        if self.count < self.capacity() {
            self.count = self.count.saturating_add(1);
        }
    }

    /// The samples, oldest first — the order a graph draws them in.
    pub fn iter_oldest_first(&self) -> impl Iterator<Item = f32> + '_ {
        // Before the buffer wraps the oldest sample is at 0; afterwards it is
        // wherever the cursor is about to overwrite.
        let start = if self.is_full() { self.cursor } else { 0 };
        step::indices(self.capacity(), start, true)
            .take(self.count)
            .map(|idx| self.samples.get(idx).copied().unwrap_or(0.0))
    }

    /// The samples, oldest first, as a `Vec`.
    #[must_use]
    pub fn to_vec(&self) -> Vec<f32> {
        self.iter_oldest_first().collect()
    }

    /// The most recently pushed sample, or `0.0` if there is none.
    #[must_use]
    pub fn latest(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        // The cursor points at the *next* slot, so the latest is one behind it.
        let idx = step::wrapping_before(self.capacity(), self.cursor);
        self.samples.get(idx).copied().unwrap_or(0.0)
    }

    /// The arithmetic mean of the recorded samples, or `0.0` if there are none.
    #[must_use]
    pub fn average(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let sum: f32 = self.iter_oldest_first().sum();
        // `count` is non-zero here, and f32 division by a non-zero value cannot
        // trap, so this is the one place the total is divided.
        sum / self.count as f32
    }

    /// The largest recorded sample, or `0.0` if there are none.
    ///
    /// This is the true maximum: a history of only negative samples reports its
    /// largest negative one. Two of the three buffers this replaces folded from
    /// zero instead, so they would have reported `0.0` for such a history — a
    /// value that was never measured. No current caller records negative
    /// samples (they are percentages and byte rates), so the fix changes no
    /// graph on screen, but the honest answer is the one worth keeping.
    #[must_use]
    pub fn peak(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        self.iter_oldest_first().fold(f32::NEG_INFINITY, f32::max)
    }
}

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // The history stores samples and hands them back untouched; the only place
    // it computes on them is `average`, which is the one assertion below that
    // compares with a tolerance. Everywhere else the exact comparison *is* the
    // assertion — a sample that came back only *nearly* the one pushed would
    // be a bug, not a rounding artefact.
    #![allow(clippy::float_cmp)]

    use super::SampleHistory;

    #[test]
    fn a_fresh_history_holds_nothing() {
        let history = SampleHistory::new(4);
        assert!(history.is_empty());
        assert!(!history.is_full());
        assert_eq!(history.len(), 0);
        assert_eq!(history.capacity(), 4);
        assert_eq!(history.to_vec(), Vec::<f32>::new());
        assert_eq!(history.latest(), 0.0);
        assert_eq!(history.average(), 0.0);
        assert_eq!(history.peak(), 0.0);
    }

    #[test]
    fn samples_come_back_in_the_order_they_went_in() {
        let mut history = SampleHistory::new(4);
        for value in [1.0, 2.0, 3.0] {
            history.push(value);
        }
        assert_eq!(history.to_vec(), vec![1.0, 2.0, 3.0]);
        assert_eq!(history.len(), 3);
        assert!(!history.is_full());
        assert_eq!(history.latest(), 3.0);
    }

    /// The point of the buffer: once it is full, the window slides.
    #[test]
    fn pushing_past_the_capacity_drops_the_oldest_sample() {
        let mut history = SampleHistory::new(3);
        for value in [1.0, 2.0, 3.0, 4.0, 5.0] {
            history.push(value);
        }
        assert!(history.is_full());
        assert_eq!(history.len(), 3);
        assert_eq!(history.to_vec(), vec![3.0, 4.0, 5.0]);
        assert_eq!(history.latest(), 5.0);
    }

    /// The window must stay correct however many times it has wrapped, and the
    /// cursor must never leave the buffer.
    #[test]
    fn the_window_is_the_last_capacity_samples_however_often_it_wraps() {
        for capacity in 1..6_usize {
            let mut history = SampleHistory::new(capacity);
            let mut pushed: Vec<f32> = Vec::new();
            for step in 0..40 {
                let value = step as f32;
                history.push(value);
                pushed.push(value);

                let expected: Vec<f32> =
                    pushed.iter().rev().take(capacity).rev().copied().collect();
                assert_eq!(
                    history.to_vec(),
                    expected,
                    "capacity={capacity} step={step}"
                );
                assert_eq!(history.latest(), value, "capacity={capacity} step={step}");
                assert_eq!(history.len(), expected.len());
            }
        }
    }

    /// A zero-capacity history is a history that holds nothing — not a panic.
    #[test]
    fn a_history_with_no_room_stays_total() {
        let mut history = SampleHistory::new(0);
        for value in [1.0, 2.0, 3.0] {
            history.push(value);
        }
        assert!(history.is_empty());
        assert!(
            history.is_full(),
            "there is no room, so there is no room left"
        );
        assert_eq!(history.to_vec(), Vec::<f32>::new());
        assert_eq!(history.latest(), 0.0);
        assert_eq!(history.average(), 0.0);
        assert_eq!(history.peak(), 0.0);
    }

    #[test]
    fn the_average_and_the_peak_cover_exactly_the_recorded_samples() {
        let mut history = SampleHistory::new(3);
        history.push(10.0);
        history.push(20.0);
        // Still partly empty: the untouched slots must not count as zeros.
        assert_eq!(history.average(), 15.0);
        assert_eq!(history.peak(), 20.0);

        history.push(30.0);
        history.push(0.0);
        assert_eq!(history.to_vec(), vec![20.0, 30.0, 0.0]);
        assert!((history.average() - 50.0 / 3.0).abs() < 1e-5);
        assert_eq!(history.peak(), 30.0);
    }

    /// Folding a maximum from zero would have reported a value never measured.
    #[test]
    fn the_peak_of_only_negative_samples_is_the_largest_negative_one() {
        let mut history = SampleHistory::new(3);
        for value in [-5.0, -1.0, -9.0] {
            history.push(value);
        }
        assert_eq!(history.peak(), -1.0);
    }

    #[test]
    fn clearing_forgets_the_samples_but_not_the_capacity() {
        let mut history = SampleHistory::new(3);
        for value in [1.0, 2.0, 3.0, 4.0] {
            history.push(value);
        }
        history.clear();

        assert!(history.is_empty());
        assert_eq!(history.capacity(), 3);
        assert_eq!(history.to_vec(), Vec::<f32>::new());

        // And it must fill up from the start again, not from where it left off.
        history.push(7.0);
        assert_eq!(history.to_vec(), vec![7.0]);
        assert_eq!(history.latest(), 7.0);
    }
}
