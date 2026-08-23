//! Making a destructive self-test safe to run on a machine that is in use.
//!
//! Nearly every state-holding module under `kernel/src/fs/` carries a
//! `self_test()` that is also reachable as a shell subcommand (`a11y test`,
//! `hotkeys test`, `theme test`, …).  The house style for those suites was to
//! open with `clear_all()` — which makes the suite idempotent, makes its
//! opening emptiness assertion true by construction, and, on a machine where
//! the table holds anything, **destroys the user's data**.  `useracct test`
//! deleted every account, group and session and then printed `all tests
//! passed`.  See `known-issues.md` →
//! `TD-A-SELFTESTS-ARE-DESTRUCTIVE-ON-A-LIVE-MACHINE`.
//!
//! At boot the tables are empty, so the wipe is a no-op and the boot test is
//! green either way.  That asymmetry is the trap: a green boot proves nothing
//! about the shell path, so the two cannot be fixed separately.
//!
//! ## The fix, and why it is this one rather than the obvious alternatives
//!
//! The suites assert *exact* table contents ("exactly two tools are
//! registered", "the first ID is 1"), and an exact assertion is only a
//! statement you can make about a table you own entirely.  Three shapes were
//! considered:
//!
//! * **Baseline-relative** — count the rows on entry and state every
//!   assertion relative to that count.  Preserves the data and keeps full
//!   coverage, but it cannot express most of these assertions: "`next_id`
//!   starts at 1" has no baseline-relative form, and rewriting ~55 suites to
//!   avoid such statements would delete most of what they check.
//! * **Decline to run** — skip the suite when the table is non-empty.  Safe,
//!   but it silently loses all coverage on exactly the machines where a user
//!   is likely to type `test` because they suspect something is wrong.
//! * **Move the live state aside** — what this module does.  The suite runs
//!   against genuinely pristine state, so every existing assertion holds
//!   unchanged and unweakened; the user's contents are put back afterwards.
//!   Full coverage, zero data loss, and idempotent by construction.
//!
//! The third only became available once each module had a named constructor
//! for its pristine state.  Extracting that constructor is worth doing on its
//! own account: the `static STATE: Mutex<State> = Mutex::new(State { … })`
//! literal and `clear_all()` were two independent spellings of "what a fresh
//! boot looks like", free to drift apart with nothing to catch it.
//!
//! ## Using it
//!
//! ```ignore
//! pub fn self_test() -> KernelResult<()> {
//!     crate::fs::selftest::with_pristine(&STATE, State::new(), self_test_inner)
//! }
//!
//! fn self_test_inner() -> KernelResult<()> { /* the suite, unchanged */ }
//! ```
//!
//! A module whose suite also depends on free-standing counters must save and
//! restore those itself, around the `with_pristine` call — the helper only
//! knows about the one table it is handed.
//!
//! ## What it deliberately does not do
//!
//! It does not catch panics.  A failing assertion in a suite is a kernel
//! panic, and a kernel that has just proved one of its own invariants wrong
//! has no business carrying the user's data forward into whatever runs next.
//! Every *non-panicking* exit — including an early `?` on a `KernelResult` —
//! does restore.

use crate::sync::{Mutex, PreemptSpinMutex};

/// A lock whose contents can be swapped wholesale.
///
/// This exists only because `kernel/src/fs/` is split between two lock types —
/// `Mutex` and `PreemptSpinMutex`, the latter usually imported *as* `Mutex`,
/// so which one a module uses is not visible at the call site.  Rather than
/// make every caller know, the helper is generic and the two impls below are
/// the whole of the difference.
pub trait PristineCell {
    /// The guarded value.
    type Value;

    /// Put `value` in and hand the previous contents back.
    fn replace_contents(&self, value: Self::Value) -> Self::Value;
}

impl<T> PristineCell for Mutex<T> {
    type Value = T;

    fn replace_contents(&self, value: T) -> T {
        core::mem::replace(&mut *self.lock(), value)
    }
}

impl<T> PristineCell for PreemptSpinMutex<T> {
    type Value = T;

    fn replace_contents(&self, value: T) -> T {
        core::mem::replace(&mut *self.lock(), value)
    }
}

/// Run `body` with `state` replaced by `pristine`, then put the original
/// contents back and return whatever `body` returned.
///
/// The lock is released for the duration of `body`, which is required rather
/// than merely polite: the suite calls the module's own public API, and that
/// API takes the same lock.
///
/// # Concurrency
///
/// A concurrent reader of `state` observes the pristine substitute while the
/// suite runs, and the real contents again afterwards.  That is strictly
/// better than the behaviour this replaces, where the same reader observed an
/// emptied table *permanently*, but it is not isolation — these suites are
/// diagnostics, not something to run under load.
pub fn with_pristine<C: PristineCell, R>(
    state: &C,
    pristine: C::Value,
    body: impl FnOnce() -> R,
) -> R {
    let saved = state.replace_contents(pristine);
    let result = body();
    // Drops the substitute, and with it every fixture the suite created.
    state.replace_contents(saved);
    result
}
