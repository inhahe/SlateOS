//! Where the time in [`ScaledFont::shape_with`] goes, phase by phase.
//!
//! [`ScaledFont::shape_with`]: crate::scaled::ScaledFont::shape_with
//!
//! Compiled to nothing unless the `phase-timing` feature is on. With it off,
//! [`Timer::start`] returns a zero-sized value with no destructor and every
//! call site optimises away entirely; with it on, each guard adds its own
//! lifetime to a thread-local total for its phase.
//!
//! # Why this exists in the tree rather than as a patch
//!
//! The breakdown it produces has been wanted twice, and the first time it was
//! taken by editing `shape_with` in place and throwing the edit away. That
//! left `known-issues.md` →
//! `C-FONT-SHAPING-IS-1400X-SLOWER-THAN-IT-SHOULD-BE` holding a table nobody
//! could reproduce without rewriting the patch — and by the time anyone
//! wanted to, the table was stale in two separate ways at once. An instrument
//! that lives in the tree costs a feature flag and answers the question in one
//! command.
//!
//! # Two rules for reading what it prints
//!
//! * **The shares are the trustworthy half, not the microseconds.** Every
//!   guard charges its own `Instant::now()` pair to the phase it is timing, so
//!   the instrumented total runs above the uninstrumented one. A factor that
//!   inflates every phase alike cancels in a ratio and does not cancel in a
//!   subtraction.
//! * **Print once, at the end.** An earlier version of this measurement
//!   printed per phase as it went and reported 490us for a 129us shape, giving
//!   four unrelated phases the same ~38us — which was the cost of a line into
//!   a captured stderr, not the work. Accumulate, then print.
//!
//! # Non-overlapping by construction
//!
//! A phase's total is the sum of its guards' lifetimes, and nothing here
//! detects nesting: two live guards would charge the same wall time twice and
//! the shares would not sum to one. The call sites in `shape_with` are laid
//! out so that at most one guard is alive at a time, and
//! [`Snapshot::unaccounted`] is what checks it — a negative remainder means
//! two phases have been made to overlap.

use crate::phase::sealed::Repr;

/// One stretch of [`shape_with`](crate::scaled::ScaledFont::shape_with),
/// timed on its own.
///
/// The divisions are the ones the pipeline already has — each is a pass that
/// needs all of the previous one's output — so a share here names a piece of
/// work that could be attacked without disturbing its neighbours.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Bidi levels, one per byte of the text.
    ByteLevels,
    /// Normalization: [`norm::pieces`](crate::norm::pieces).
    Norm,
    /// Korean preprocessing, variation sequences, per-piece levels and
    /// mirroring, the script/direction run split, and the legacy-Thai pass.
    PreScript,
    /// Cursive joining forms.
    Joining,
    /// The piece loop that turns characters into `SubGlyph`s, `cmap` included.
    GlyphBuild,
    /// Substitution: `GSUB`.
    Gsub,
    /// Per-segment bookkeeping between the two layout passes — which segments
    /// want the legacy kern table, which want the measuring fallback, which
    /// zero their marks' advances.
    SegPrep,
    /// One "is this glyph a combining mark?" per glyph, asked of the face's
    /// `GDEF`.
    ///
    /// Split out from [`SegPrep`](Self::SegPrep), which it sits next to and
    /// used to be counted with, because it is the only part of that stretch
    /// whose cost is per *glyph* rather than per segment — and so the only
    /// part that a long line makes expensive.
    Marks,
    /// One `hmtx` advance per glyph.
    Advances,
    /// Positioning: `GPOS`.
    Gpos,
    /// Kerning, mark synthesis, reordering, and building the output run.
    Tail,
}

impl Phase {
    /// How many phases there are.
    pub const COUNT: usize = 11;

    /// Every phase, in pipeline order.
    pub const ALL: [Self; Self::COUNT] = [
        Self::ByteLevels,
        Self::Norm,
        Self::PreScript,
        Self::Joining,
        Self::GlyphBuild,
        Self::Gsub,
        Self::SegPrep,
        Self::Marks,
        Self::Advances,
        Self::Gpos,
        Self::Tail,
    ];

    /// A short name for a table column.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ByteLevels => "byte_levels",
            Self::Norm => "norm",
            Self::PreScript => "pre+script",
            Self::Joining => "joining",
            Self::GlyphBuild => "glyphbuild",
            Self::Gsub => "gsub",
            Self::SegPrep => "segprep",
            Self::Marks => "marks",
            Self::Advances => "advances",
            Self::Gpos => "gpos",
            Self::Tail => "tail",
        }
    }

    /// Index into the thread-local totals.
    const fn slot(self) -> usize {
        match self {
            Self::ByteLevels => 0,
            Self::Norm => 1,
            Self::PreScript => 2,
            Self::Joining => 3,
            Self::GlyphBuild => 4,
            Self::Gsub => 5,
            Self::SegPrep => 6,
            Self::Marks => 7,
            Self::Advances => 8,
            Self::Gpos => 9,
            Self::Tail => 10,
        }
    }
}

/// Times one phase, from construction to drop.
///
/// Held in a `let` binding whose lifetime is the stretch being measured. Where
/// that stretch is a run of statements rather than a block, the call site ends
/// it with an explicit `drop`, which is why this is `#[must_use]`: a guard
/// bound to `_` would be dropped immediately and silently charge nothing.
#[must_use = "the guard's lifetime is the measurement; binding it to `_` measures nothing"]
pub struct Timer {
    /// Held only for its destructor, which is what charges the phase — hence
    /// the underscore, which is what tells `dead_code` that a field nobody
    /// reads is the point here rather than an oversight.
    _repr: Repr,
}

impl Timer {
    /// Starts timing `phase`.
    #[inline]
    pub fn start(phase: Phase) -> Self {
        Self {
            _repr: Repr::start(phase),
        }
    }
}

/// The totals of one shaping, in nanoseconds per phase.
///
/// Read out with [`snapshot`]; the caller is expected to [`reset`] before each
/// shaping it wants measured separately.
#[derive(Clone, Copy)]
pub struct Snapshot {
    /// Per-phase nanoseconds, indexed by [`Phase::slot`].
    ns: [u64; Phase::COUNT],
    /// Nanoseconds the whole shaping took, timed by the caller.
    total_ns: u64,
}

impl Snapshot {
    /// What one phase cost, in nanoseconds.
    #[must_use]
    pub fn get(&self, phase: Phase) -> u64 {
        self.ns.get(phase.slot()).copied().unwrap_or(0)
    }

    /// What the shaping cost in total, in nanoseconds.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total_ns
    }

    /// Time inside the shaping that no phase claimed.
    ///
    /// Saturating rather than signed because the honest answer to a negative
    /// remainder is "the call sites are wrong", not a number: two overlapping
    /// guards charge the same wall time twice, and a zero here is the loudest
    /// way to say the sum has stopped meaning anything. A *positive* value is
    /// ordinary — it is the glue between phases plus the guards' own cost.
    #[must_use]
    pub fn unaccounted(&self) -> u64 {
        let claimed: u64 = self.ns.iter().copied().sum();
        self.total_ns.saturating_sub(claimed)
    }

    /// The elementwise smaller of two snapshots.
    ///
    /// The statistic every figure in the shaping instrument reports, applied
    /// per phase: noise on a desktop is one-sided — nothing can make a phase
    /// finish sooner than the code allows — so across many shapings the
    /// smallest sample of each phase is the best estimate of what that phase
    /// costs, and everything above it is a measurement of the operating
    /// system. Taken per phase rather than by picking the fastest *shaping*
    /// wholesale, because a single shaping that is quiet in every phase at
    /// once is rarer than a quiet phase, and the phases do not have to be
    /// quiet together to each be measured.
    #[must_use]
    pub fn min(mut self, other: Self) -> Self {
        for (mine, theirs) in self.ns.iter_mut().zip(other.ns.iter()) {
            *mine = (*mine).min(*theirs);
        }
        self.total_ns = self.total_ns.min(other.total_ns);
        self
    }

    /// A snapshot that loses every `min` against a real one.
    #[must_use]
    pub const fn worst() -> Self {
        Self {
            ns: [u64::MAX; Phase::COUNT],
            total_ns: u64::MAX,
        }
    }
}

/// Zeroes the thread-local totals, ready for one shaping to be measured.
pub fn reset() {
    sealed::reset();
}

/// Reads the thread-local totals back out.
///
/// `total_ns` is the caller's own timing of the whole shaping, which this
/// module cannot take for itself: the outermost thing worth timing is the call
/// to `shape_with`, and that is on the far side of the boundary.
#[must_use]
pub fn snapshot(total_ns: u64) -> Snapshot {
    Snapshot {
        ns: sealed::totals(),
        total_ns,
    }
}

#[cfg(feature = "phase-timing")]
mod sealed {
    // The initializer below is already a `const { … }` block — exactly the
    // form `missing_const_for_thread_local` asks for — but the lint fires
    // anyway on rust-1.95, because it does not recognise the form the macro
    // emits. Checked with a scalar `Cell<u64> = const { Cell::new(0) }`, which
    // it also rejects, so this is the lint and not the code. Suppressed at
    // module level because an `#[allow]` on a macro invocation is ignored.
    // Same suppression, same reason, in `guitk`'s `signal.rs` and the
    // compositor's `present/host.rs`.
    #![allow(clippy::missing_const_for_thread_local)]

    use super::Phase;
    use std::cell::Cell;
    use std::time::Instant;

    thread_local! {
        /// Nanoseconds charged to each phase since the last [`reset`].
        ///
        /// Thread-local rather than global because two threads shaping at once
        /// would otherwise sum into each other, and the instrument's whole
        /// premise is that one shaping at a time is being measured.
        ///
        /// [`reset`]: super::reset
        static TOTALS: Cell<[u64; Phase::COUNT]> = const { Cell::new([0; Phase::COUNT]) };
    }

    /// A live measurement: which phase, and when it started.
    pub(super) struct Repr {
        phase: Phase,
        start: Instant,
    }

    impl Repr {
        #[inline]
        pub(super) fn start(phase: Phase) -> Self {
            Self {
                phase,
                start: Instant::now(),
            }
        }
    }

    impl Drop for Repr {
        fn drop(&mut self) {
            let ns = u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX);
            let slot = self.phase.slot();
            TOTALS.with(|totals| {
                let mut all = totals.get();
                if let Some(cell) = all.get_mut(slot) {
                    *cell = cell.saturating_add(ns);
                }
                totals.set(all);
            });
        }
    }

    pub(super) fn reset() {
        TOTALS.with(|totals| totals.set([0; Phase::COUNT]));
    }

    pub(super) fn totals() -> [u64; Phase::COUNT] {
        TOTALS.with(Cell::get)
    }
}

#[cfg(not(feature = "phase-timing"))]
mod sealed {
    use super::Phase;

    /// Nothing at all: no field, no destructor, no code.
    pub(super) struct Repr;

    impl Repr {
        #[inline]
        pub(super) const fn start(_phase: Phase) -> Self {
            Self
        }
    }

    pub(super) const fn reset() {}

    pub(super) const fn totals() -> [u64; Phase::COUNT] {
        [0; Phase::COUNT]
    }
}
