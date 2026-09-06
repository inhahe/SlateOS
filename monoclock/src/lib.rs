//! Monotonic-clock instants and spans, with the unit in the type.
//!
//! # Why this crate exists
//!
//! On 2026-09-05 three of the four programs in this tree that read the
//! monotonic clock had the same defect, written independently, sharing no code:
//!
//! | Program | Wrapper was called | Syscall actually returns | Consequence |
//! |---|---|---|---|
//! | `userspace/sshd` | `clock_monotonic_ms` | nanoseconds | `LoginGraceTime` expired 120 µs into a 120-second grace. **No client could authenticate at all.** |
//! | `userspace/inetd` | `clock_monotonic_ms` | nanoseconds | The per-source `MaxRate` window was 60 µs instead of 60 s, so the rate limiter never once fired. |
//! | `userspace/dig` | `clock_monotonic_us` | nanoseconds | Harmless — the value was only hashed — but it was being hashed into a DNS transaction ID, which was a separate and worse bug. |
//!
//! Every one of them was a `u64` that a human had named after a unit it did not
//! have, then divided or subtracted by a constant chosen for the unit in the
//! name. **A unit carried only in an identifier is a comment the compiler does
//! not read.** The two programs that got it right — `init` and `ticker` — did so
//! by suffixing every single variable `_ns` and never claiming otherwise, which
//! is discipline, and discipline is what runs out.
//!
//! This crate makes the mistake unrepresentable. A clock reading is an
//! [`Instant`], a span is an [`Elapsed`], and there is no operation that turns
//! one into a bare integer except one named for the unit it yields. Subtracting
//! an `Instant` from an `Instant` gives an `Elapsed`; comparing an `Elapsed`
//! against a configured number of seconds requires [`Elapsed::from_secs`],
//! which is the conversion, in one visible place, spelled out.
//!
//! # What this crate deliberately does *not* do
//!
//! **It does not issue the syscall.** Each program keeps its own
//! `SYS_CLOCK_MONOTONIC` shim and wraps the result with
//! [`Instant::from_nanos_since_boot`]. That looks like a missed opportunity to
//! deduplicate and is not one:
//!
//! - The programs that need this are not alike. `sshd` and `inetd` are `std`
//!   userspace daemons; `init` and `ticker` are `no_std`, `no_main` bare-metal
//!   binaries carrying hand-written inline-asm shims for their whole syscall
//!   surface — `mmap`, `spawn`, `wait`, console I/O. Lifting only the clock out
//!   of thirty such shims would not reduce the duplication, it would relocate a
//!   thirtieth of it.
//! - A crate that made the syscall would have to guess how to behave off-target,
//!   and the callers disagree for good reasons: `sshd` wants `None` so it can
//!   refuse to enforce a timeout it cannot measure, while `init` prints an
//!   uptime and wants a number.
//! - Reaching the kernel from a dependency is the exact shape of a bug this tree
//!   has already had. `posix` gates its syscalls `#[cfg(target_os = "none")]`
//!   for the bare-metal `libc.a` build, so a program that depends on `posix` as
//!   an rlib links a second libc in which every syscall answers `-ENOSYS`. See
//!   `known-issues.md`
//!   → `TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT`.
//!
//! So the boundary is drawn where the value's *meaning* is asserted rather than
//! where it is fetched. `Instant::from_nanos_since_boot` is the one line per
//! program that says "the syscall returns nanoseconds"; everything downstream of
//! it is checked by the compiler.
//!
//! # Example
//!
//! ```
//! use monoclock::{Elapsed, Instant};
//!
//! // What each program's own syscall shim produces.
//! let connected = Instant::from_nanos_since_boot(5_000_000_000);
//! let now = Instant::from_nanos_since_boot(5_010_000_000);
//!
//! // A grace period written in seconds, as configuration files write it.
//! let grace = Elapsed::from_secs(120);
//!
//! assert!(now.saturating_since(connected) < grace);
//! ```
//!
//! Note what cannot be written: there is no way to compare `now` against
//! `connected` and `120` without saying which of them is a span and what unit
//! the `120` is in.

#![no_std]

/// Nanoseconds in a second.
const NANOS_PER_SEC: u64 = 1_000_000_000;

/// Nanoseconds in a millisecond.
const NANOS_PER_MILLI: u64 = 1_000_000;

/// Nanoseconds in a microsecond.
const NANOS_PER_MICRO: u64 = 1_000;

/// A reading of the system monotonic clock.
///
/// The value is nanoseconds since boot, but that is this type's business rather
/// than its callers'. An `Instant` is only useful in relation to another
/// `Instant`: the epoch is an arbitrary point (the machine powering on) and
/// means nothing across a reboot, so an absolute reading carries no information
/// on its own. That is why the only arithmetic offered is
/// [`saturating_since`](Self::saturating_since) and
/// [`saturating_add`](Self::saturating_add) — a difference and an offset, both
/// yielding types that say what they are.
///
/// `Ord` is derived, and is the one comparison that is meaningful between two
/// readings of the same clock: "which came first".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(u64);

impl Instant {
    /// The zero of this clock: the moment the machine started.
    ///
    /// It is a real, nameable point rather than a sentinel — it is where
    /// [`saturating_sub`](Self::saturating_sub) stops, and it is what a caller
    /// that cannot read the clock at all may reasonably substitute when the
    /// conservative answer is "as long ago as possible" (a sliding window then
    /// contains everything, rather than nothing). Callers for which the
    /// conservative answer is the opposite should return `Option` and refuse,
    /// not fall back to this.
    pub const BOOT: Self = Self(0);

    /// Wrap a raw `SYS_CLOCK_MONOTONIC` result.
    ///
    /// This is the assertion "the number I just got from the kernel is
    /// nanoseconds since boot", and it should appear exactly once per program,
    /// immediately around the syscall. It is named at length so that a reader
    /// checking that claim against `kernel/src/syscall/number.rs` can see what
    /// is being claimed without leaving the line.
    ///
    /// The inverse is [`as_nanos_since_boot`](Self::as_nanos_since_boot), for
    /// the cases that genuinely need the scalar back — a log line, a value
    /// stored in a table that outlives the type, a hash input.
    #[must_use]
    pub const fn from_nanos_since_boot(nanos: u64) -> Self {
        Self(nanos)
    }

    /// The raw reading, in nanoseconds since boot.
    #[must_use]
    pub const fn as_nanos_since_boot(self) -> u64 {
        self.0
    }

    /// How long has passed between `earlier` and `self`.
    ///
    /// Saturating at zero rather than wrapping or panicking. A monotonic clock
    /// that steps backwards is a kernel bug, but the code asking this question
    /// is often code an unauthenticated peer can reach — `sshd`'s login grace
    /// timer, `inetd`'s rate limiter — and turning a kernel bug into a panic
    /// there converts a wrong number into a denial of service. Reporting "no
    /// time has passed" is the conservative reading: a timeout does not fire, a
    /// rate window does not empty.
    #[must_use]
    pub const fn saturating_since(self, earlier: Self) -> Elapsed {
        Elapsed(self.0.saturating_sub(earlier.0))
    }

    /// The instant `span` after this one, saturating at the end of the clock.
    #[must_use]
    pub const fn saturating_add(self, span: Elapsed) -> Self {
        Self(self.0.saturating_add(span.0))
    }

    /// The instant `span` before this one, saturating at boot.
    ///
    /// Saturating at boot is what makes a sliding window behave correctly in
    /// the first moments after start-up: a window that reaches back further
    /// than the machine has been running covers everything there is, which is
    /// the right answer, rather than wrapping to a cutoff near `u64::MAX` that
    /// would discard every timestamp.
    #[must_use]
    pub const fn saturating_sub(self, span: Elapsed) -> Self {
        Self(self.0.saturating_sub(span.0))
    }
}

/// A span of time between two [`Instant`]s.
///
/// Constructed either by subtracting two instants or from a figure in the unit
/// a human wrote it in — [`from_secs`](Self::from_secs) for a configuration
/// value, [`from_millis`](Self::from_millis) for a protocol timeout. Read back
/// the same way. There is no `Elapsed -> u64` that does not name a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Elapsed(u64);

impl Elapsed {
    /// A span of zero.
    pub const ZERO: Self = Self(0);

    /// A span of `secs` seconds — the unit configuration files are written in.
    ///
    /// Saturating rather than wrapping, so a nonsensically large configured
    /// value becomes "effectively forever" rather than "very nearly nothing",
    /// which is the direction that fails safe for a timeout.
    #[must_use]
    pub const fn from_secs(secs: u64) -> Self {
        Self(secs.saturating_mul(NANOS_PER_SEC))
    }

    /// A span of `millis` milliseconds.
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis.saturating_mul(NANOS_PER_MILLI))
    }

    /// A span of `micros` microseconds.
    #[must_use]
    pub const fn from_micros(micros: u64) -> Self {
        Self(micros.saturating_mul(NANOS_PER_MICRO))
    }

    /// A span of `nanos` nanoseconds.
    #[must_use]
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    /// Whole seconds in this span, rounding down.
    #[must_use]
    pub const fn as_secs(self) -> u64 {
        self.0 / NANOS_PER_SEC
    }

    /// Whole milliseconds in this span, rounding down.
    #[must_use]
    pub const fn as_millis(self) -> u64 {
        self.0 / NANOS_PER_MILLI
    }

    /// Whole microseconds in this span, rounding down.
    #[must_use]
    pub const fn as_micros(self) -> u64 {
        self.0 / NANOS_PER_MICRO
    }

    /// This span in nanoseconds.
    #[must_use]
    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// The sub-second part of this span, in whole milliseconds.
    ///
    /// For printing an uptime as `1234.567s` without the caller doing modular
    /// arithmetic on a nanosecond count — which is the shape of the code this
    /// crate exists to remove.
    #[must_use]
    pub const fn subsec_millis(self) -> u64 {
        (self.0 % NANOS_PER_SEC) / NANOS_PER_MILLI
    }
}

#[cfg(test)]
mod tests {
    // A test that panics on bad data is a test doing its job.
    #![allow(clippy::arithmetic_side_effects)]

    use super::{Elapsed, Instant};

    /// The regression test for the whole class, stated in its own terms.
    ///
    /// `sshd` compared a nanosecond difference against a seconds figure and
    /// concluded that 10 ms exceeded 120 s. Written through these types, that
    /// comparison has the right answer and the wrong one cannot be spelled.
    #[test]
    fn ten_milliseconds_is_not_more_than_a_two_minute_grace_period() {
        let start = Instant::from_nanos_since_boot(5_000_000_000);
        let now = start.saturating_add(Elapsed::from_millis(10));

        assert!(now.saturating_since(start) < Elapsed::from_secs(120));
    }

    #[test]
    fn a_span_just_over_the_limit_exceeds_it_and_one_just_under_does_not() {
        let start = Instant::BOOT;
        let grace = Elapsed::from_secs(120);
        let since_start = |span| start.saturating_add(span).saturating_since(start);

        assert!(since_start(Elapsed::from_secs(119)) < grace);
        assert!(since_start(Elapsed::from_secs(121)) > grace);
        // Exactly at the limit has not *exceeded* it. Every timeout built on
        // this type fires on `>`, so the boundary instant is still inside the
        // allowance -- a grace period of 120 seconds grants the whole 120th.
        assert!(since_start(grace) <= grace);
    }

    #[test]
    fn a_backwards_clock_step_reports_no_time_passed_rather_than_a_huge_span() {
        let later = Instant::from_nanos_since_boot(10 * 1_000_000_000);
        let earlier = Instant::from_nanos_since_boot(20 * 1_000_000_000);

        assert_eq!(later.saturating_since(earlier), Elapsed::ZERO);
    }

    /// A sliding window in the first seconds after boot must not wrap.
    ///
    /// `inetd` computes its rate-window cutoff as `now - window`. Before the
    /// machine has been up for a whole window that would wrap to a cutoff near
    /// `u64::MAX`, discarding every timestamp — the limiter silently switching
    /// off for its first minute of life, which is when a flood is most likely.
    #[test]
    fn a_window_reaching_back_past_boot_covers_everything_rather_than_nothing() {
        let now = Instant::from_nanos_since_boot(Elapsed::from_secs(5).as_nanos());
        let cutoff = now.saturating_sub(Elapsed::from_secs(60));

        assert_eq!(cutoff, Instant::BOOT);
        assert!(Instant::from_nanos_since_boot(1) >= cutoff);
    }

    /// Boot is the zero of the clock, not a value apart from it.
    ///
    /// A caller that substitutes [`Instant::BOOT`] for an unreadable clock is
    /// relying on it comparing as earlier than every real reading, and on the
    /// span since it being the whole uptime.
    #[test]
    fn boot_is_the_earliest_reading_and_the_origin_of_uptime() {
        let five_seconds_up = Instant::from_nanos_since_boot(5_000_000_000);

        assert!(Instant::BOOT < five_seconds_up);
        assert_eq!(Instant::BOOT.as_nanos_since_boot(), 0);
        assert_eq!(
            five_seconds_up.saturating_since(Instant::BOOT),
            Elapsed::from_secs(5),
            "the span since boot is the uptime"
        );
    }

    #[test]
    fn unit_conversions_round_trip_and_round_down() {
        assert_eq!(Elapsed::from_secs(3).as_millis(), 3_000);
        assert_eq!(Elapsed::from_millis(1500).as_secs(), 1);
        assert_eq!(Elapsed::from_millis(1500).subsec_millis(), 500);
        assert_eq!(Elapsed::from_micros(1).as_nanos(), 1_000);
        assert_eq!(Elapsed::from_nanos(999).as_micros(), 0);
    }

    /// A configured value large enough to overflow becomes "forever", not "now".
    ///
    /// The direction matters: a timeout that saturates to a huge span never
    /// fires, while one that wrapped to a small span would fire immediately —
    /// which is precisely the failure `sshd` had, where every connection was
    /// disconnected during authentication.
    #[test]
    fn an_absurd_configured_span_fails_towards_never_expiring() {
        let huge = Elapsed::from_secs(u64::MAX);
        assert_eq!(huge.as_nanos(), u64::MAX);
        assert!(huge > Elapsed::from_secs(100 * 365 * 24 * 60 * 60));
    }
}
