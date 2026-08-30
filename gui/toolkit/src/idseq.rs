//! A source of identifiers that are never handed out twice.
//!
//! Fourteen types in the desktop shell needed one of these, and each wrote
//! its own: a `next_id` field, `let id = self.next_id;`, and a line that
//! advances it. The three lines are trivial. Which is why nobody looked at
//! them, and why the fourteen copies gave **four different answers** to the
//! only question that matters — what happens when the counter runs out:
//!
//! | What was written | What it does at the top |
//! |---|---|
//! | `self.next_id += 1` (6 copies) | panics in a debug build, wraps in a release one |
//! | `saturating_add(1)` (6 copies) | hands out `MAX` forever |
//! | `wrapping_add(1)` (3 copies) | hands out `0`, `1`, `2` … again |
//! | `checked_add(1)?` (2 copies) | refuses to allocate |
//!
//! Three of those four are the same bug wearing different clothes. Wrapping
//! is the worst: the ids it reuses first are the *lowest* ones, which in a
//! shell that has been up long enough to wrap are overwhelmingly the ones
//! still alive — the first workspace, the pinned window, the notification
//! that has been sitting in the tray since boot. A duplicate id is not a
//! crash; it is one object silently answering to another object's name, which
//! is how a dismissal dismisses the wrong notification.
//!
//! # Why there are two ways to take an id
//!
//! An id sequence has exactly one contract — *never the same value twice* —
//! and the honest primitive is therefore [`IdSeq::issue`], which returns
//! `None` once there is nothing left to give.
//!
//! But most callers have no failure path to give that `None` to, and inventing
//! one is not a service to anybody: `add_icon` cannot usefully tell its caller
//! "the desktop has run out of numbers." For those, the fix is not an error
//! path — it is an id space so large that running out is not a thing that
//! happens. [`IdSeq::issue_infallible`] returns `T` directly, and is available
//! only where `T: Inexhaustible` — a space of at least 2^64 values, which at
//! one id per nanosecond takes 584 years to walk. The compiler, not a comment,
//! is what stops that shortcut being taken on a 32-bit counter that a
//! long-running shell really could exhaust.
//!
//! ```
//! use guitk::idseq::IdSeq;
//!
//! let mut ids: IdSeq = IdSeq::new();
//! assert_eq!(ids.issue_infallible(), 1);
//! assert_eq!(ids.issue_infallible(), 2);
//!
//! let mut small: IdSeq<u8> = IdSeq::starting_at(u8::MAX);
//! assert_eq!(small.issue(), Some(u8::MAX));
//! assert_eq!(small.issue(), None);
//! ```

/// An integer that can be used as an identifier by [`IdSeq`].
///
/// Implemented for the unsigned integers. The first id is `1`, not `0`,
/// because `0` is a sentinel in enough of this codebase's id types that
/// handing it out as a real id would be a trap of its own.
pub trait SeqId: Copy + Eq {
    /// The first id a fresh sequence issues.
    const FIRST: Self;

    /// The highest id the type can hold.
    const LAST: Self;

    /// The id after this one, or `None` if this was the last.
    #[must_use]
    fn next_after(self) -> Option<Self>;
}

/// A [`SeqId`] whose space is too large for a running program to exhaust.
///
/// Implemented for `u64` and `u128` only. The bound is the point: a sequence
/// over one of these can hand out an id per nanosecond for 584 years before
/// repeating itself, so [`IdSeq::issue_infallible`] is offered *only* here.
/// A `u32` sequence has 4.29 billion values, which a desktop that allocates
/// one per frame walks through in under a year — so it does not get the
/// shortcut, and the compiler is what says so.
///
/// This is a marker with no items. Do not implement it for a narrower type
/// to silence a compile error; widen the id instead. That error is the
/// design working.
pub trait Inexhaustible: SeqId {}

macro_rules! impl_seq_id {
    ($($t:ty),+ $(,)?) => {$(
        impl SeqId for $t {
            const FIRST: Self = 1;
            const LAST: Self = Self::MAX;

            fn next_after(self) -> Option<Self> {
                self.checked_add(1)
            }
        }
    )+};
}

impl_seq_id!(u8, u16, u32, u64, u128, usize);

impl Inexhaustible for u64 {}
impl Inexhaustible for u128 {}

/// A monotonic source of identifiers.
///
/// Issues `1`, `2`, `3`, … and then, once the underlying integer has no more
/// values, nothing at all. It never repeats itself — see the module docs for
/// why that is the whole point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IdSeq<T = u64> {
    /// The next id to issue, or `None` once the sequence is exhausted.
    next: Option<T>,
}

impl<T: SeqId> IdSeq<T> {
    /// A sequence that will issue [`SeqId::FIRST`] next.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next: Some(T::FIRST),
        }
    }

    /// A sequence that will issue `first` next.
    ///
    /// For resuming a sequence across a restart: pass one past the highest id
    /// that was persisted, so no id is handed out twice across the boundary.
    #[must_use]
    pub const fn starting_at(first: T) -> Self {
        Self { next: Some(first) }
    }

    /// A fresh id, or `None` if the sequence has no values left.
    ///
    /// Named `issue` rather than `next` so that it cannot be mistaken for —
    /// or lint-nagged towards — [`Iterator::next`]. An `IdSeq` is not an
    /// iterator: iterators walk things that already exist, and every value
    /// this yields is one that did not exist until it was asked for.
    pub fn issue(&mut self) -> Option<T> {
        let id = self.next?;
        self.next = id.next_after();
        Some(id)
    }

    /// The id [`issue`](Self::issue) would return, without consuming it.
    #[must_use]
    pub const fn peek(&self) -> Option<T> {
        self.next
    }

    /// Whether the sequence has run out of values.
    #[must_use]
    pub const fn is_exhausted(&self) -> bool {
        self.next.is_none()
    }
}

impl<T: Inexhaustible> IdSeq<T> {
    /// A fresh id, for a sequence that cannot run out.
    ///
    /// The `T: Inexhaustible` bound is the whole safety argument: the space
    /// holds at least 2^64 ids, so a program that issued one every nanosecond
    /// from the moment it started would need 584 years of doing nothing else
    /// to reach the end. There is no reachable `None` to return, which is why
    /// this returns `T` and asks the caller nothing.
    ///
    /// Should the impossible happen anyway, this repeats the *last* id rather
    /// than restarting at the first — the least harmful duplicate available,
    /// since the low ids in a long-lived shell are the ones still in use.
    /// That is a backstop, not a behaviour to rely on; if a sequence can
    /// plausibly reach its end, it is the wrong width, and [`issue`] is the
    /// method for it.
    ///
    /// [`issue`]: Self::issue
    pub fn issue_infallible(&mut self) -> T {
        self.issue().unwrap_or(T::LAST)
    }
}

impl<T: SeqId> Default for IdSeq<T> {
    fn default() -> Self {
        Self::new()
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

    use super::*;
    use std::collections::HashSet;

    #[test]
    fn a_fresh_sequence_counts_from_one() {
        let mut ids: IdSeq = IdSeq::new();
        assert_eq!(ids.peek(), Some(1));
        assert_eq!(ids.issue(), Some(1));
        assert_eq!(ids.issue(), Some(2));
        assert_eq!(ids.issue_infallible(), 3);
        assert_eq!(ids.peek(), Some(4));
        assert!(!ids.is_exhausted());
    }

    /// The contract, stated as a test rather than as a sentence.
    #[test]
    fn no_id_is_ever_issued_twice() {
        let mut ids: IdSeq<u16> = IdSeq::new();
        let mut seen = HashSet::new();
        while let Some(id) = ids.issue() {
            assert!(seen.insert(id), "{id} was issued a second time");
        }
        // `u16` has 65536 values and the sequence starts at 1, so it issues
        // all of them but zero.
        assert_eq!(seen.len(), 65_535);
        assert!(seen.contains(&u16::MAX));
        assert!(!seen.contains(&0));
    }

    /// What the six `saturating_add` copies and the three `wrapping_add`
    /// copies got wrong: at the top of the range they kept answering. A
    /// sequence with nothing left says so, and keeps saying so.
    #[test]
    fn an_exhausted_sequence_refuses_rather_than_repeating() {
        let mut ids = IdSeq::starting_at(u8::MAX);
        assert!(!ids.is_exhausted());
        assert_eq!(ids.issue(), Some(u8::MAX));
        assert!(ids.is_exhausted());
        for _ in 0..4 {
            assert_eq!(ids.issue(), None);
            assert_eq!(ids.peek(), None);
        }
    }

    /// `issue_infallible` is reachable only for a space that cannot run out,
    /// so this is the behaviour of a state no program can get to — asserted
    /// anyway, because "unreachable" is a claim and this is the check on it.
    #[test]
    fn the_infallible_backstop_repeats_the_last_id_not_the_first() {
        let mut ids: IdSeq<u64> = IdSeq::starting_at(u64::MAX);
        assert_eq!(ids.issue_infallible(), u64::MAX);
        assert!(ids.is_exhausted());
        assert_eq!(ids.issue_infallible(), u64::MAX);
        // Not 1 — the low ids are the ones a long-lived shell is still using.
        assert_ne!(ids.issue_infallible(), 1);
    }

    #[test]
    fn a_sequence_can_be_resumed_where_a_previous_run_left_off() {
        let mut before: IdSeq = IdSeq::new();
        let last = std::iter::from_fn(|| before.issue()).nth(9).unwrap();
        assert_eq!(last, 10);

        let mut after = IdSeq::starting_at(last + 1);
        assert_eq!(after.issue(), Some(11));
    }

    #[test]
    fn the_default_sequence_is_a_fresh_one() {
        let mut ids: IdSeq<u32> = IdSeq::default();
        assert_eq!(ids.issue(), Some(1));
    }
}
