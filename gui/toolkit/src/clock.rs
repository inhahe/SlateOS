//! What a component tells the time by: the machine's clock, or -- for a
//! test that steps through time, or a program drawing frames at a time of
//! its own (a recording) -- its ticks.
//!
//! A tick ([`Event::Tick`]) says how long it has been since the one before
//! it, and comes only while its program has something moving. So a
//! component that took a tick's `elapsed_ms` for the time saw time stand
//! still at the length of a frame, and one that counted ticks alone would
//! see none pass while nothing moved -- and either way, a type-ahead that
//! should start again after a pause never did. Time is the machine's,
//! unless a component is told to tell it by its ticks.
//!
//! [`Event::Tick`]: crate::event::Event::Tick

use std::time::Instant;

/// What a component tells the time by. See the module docs.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Clock {
    /// The machine's, from when the clock was made: time passes whether or
    /// not its program sends ticks.
    Real(Instant),
    /// Its ticks', in milliseconds: time passes only as [`Event::Tick`]s say
    /// -- what a test steps through, a frame at a time.
    ///
    /// [`Event::Tick`]: crate::event::Event::Tick
    Ticks(f64),
}

impl Clock {
    /// The machine's clock, starting now.
    pub(crate) fn real() -> Self {
        Self::Real(Instant::now())
    }

    /// A clock of ticks, starting at nought.
    pub(crate) const fn ticks() -> Self {
        Self::Ticks(0.0)
    }

    /// Milliseconds since the clock started.
    pub(crate) fn now_ms(&self) -> f64 {
        match self {
            Self::Real(epoch) => epoch.elapsed().as_secs_f64() * 1000.0,
            Self::Ticks(ms) => *ms,
        }
    }

    /// A tick of `elapsed_ms` -- the time since the tick before: that much
    /// more time, for a clock of ticks; the machine's keeps its own.
    pub(crate) fn tick(&mut self, elapsed_ms: u64) {
        if let Self::Ticks(ms) = self {
            // Exact below 2^53 ms -- some 285,000 years of ticks.
            #[allow(clippy::cast_precision_loss)]
            let elapsed = elapsed_ms as f64;
            *ms += elapsed;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use super::*;

    /// **A clock of ticks adds each tick's time to the time before**: a
    /// tick says how long since the last, not what the time is.
    #[test]
    fn ticks_add_up() {
        let mut clock = Clock::ticks();
        assert_eq!(clock.now_ms(), 0.0);
        clock.tick(16);
        clock.tick(16);
        clock.tick(600);
        assert_eq!(clock.now_ms(), 632.0);
    }

    /// **The machine's clock goes on whatever the ticks say**, and is not
    /// moved by them.
    #[test]
    fn the_machines_clock_keeps_its_own_time() {
        let mut clock = Clock::real();
        let before = clock.now_ms();
        clock.tick(1_000_000);
        let after = clock.now_ms();
        assert!(after >= before, "{before} then {after}");
        assert!(after < 1_000_000.0, "a tick does not move it: {after}");
    }
}
