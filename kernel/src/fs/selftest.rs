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
//! The third needs a pristine value to install, and there is exactly one
//! spelling of it that cannot be wrong: **the `static`'s own initialiser**.  A
//! `static` must be const-initialised, so its initialiser *is* "what a fresh
//! boot holds" — by definition, not by anyone's recollection.  Lift it
//! verbatim and there is never a second copy to drift.
//!
//! An earlier plan gave each module a named constructor instead.  That is one
//! spelling too many, and the hazard is not hypothetical: `crate::vmguest`
//! already carried a `State::new` that nothing called and that *had* drifted
//! from the literal beside it — it filled `features` with an entry per
//! `GuestFeature`, where the static leaves the vector empty for `init` to
//! fill.  A suite handed that constructor would have been testing a state no
//! boot ever produces, and reporting success.
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
//! A module whose state does not all live in that one table needs
//! [`pristine_atomic`] as well, for each free-standing atomic:
//!
//! ```ignore
//! pub fn self_test() {
//!     let _init = selftest::pristine_atomic(&INITIALIZED, false);
//!     let _sent = selftest::pristine_atomic(&QUERIES_SENT, 0);
//!     selftest::with_pristine(&STATE, MdnsState::new(), self_test_inner)
//! }
//! ```
//!
//! Note that this *installs* the pristine value rather than only restoring
//! the old one afterwards, and the difference is not cosmetic. `net::mdns`
//! guards `init()` with `if INITIALIZED.load(..) { return; }`. Swap the table
//! for a fresh one and leave the flag alone, and on a machine where mDNS is
//! already up the suite gets an empty table that `init()` then declines to
//! populate — no socket, no hostname — and tests nothing while reporting
//! success. Whatever gates the module's initialisation has to be made
//! pristine alongside the thing it gates.
//!
//! ## Size — when to use [`with_pristine_swapped`] instead
//!
//! [`with_pristine`] takes the pristine value by value and hands the saved one
//! back the same way, so two whole `T`s occupy the caller's frame for the
//! length of the suite.  The kernel task stack is 64 KiB
//! (`crate::sched::task::TASK_STACK_SIZE`).  That is ample for the `Vec`-backed
//! `State`s this is mostly used on — across the 25 tables converted so far the
//! largest is 1280 bytes — and fatal for `crate::net::bridge`'s
//! `[Bridge; MAX_BRIDGES]`, which is **105 216 bytes**: the swap alone would
//! overrun the stack three times over before the suite reached an assertion.
//!
//! Anything of that order wants [`with_pristine_swapped`], which never
//! materialises a `T` in a frame at all.  Note that clippy's
//! `large_stack_arrays` catches the array-shaped cases and says nothing about
//! a struct that merely *contains* a big array, so measure rather than assume;
//! `build/size_probe.py` does it for every converted table in one build.
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

    /// Exchange the contents with `other`, in place.
    ///
    /// The distinction from [`replace_contents`](Self::replace_contents) is
    /// stack footprint, not behaviour: `core::mem::swap` on a large type
    /// compiles to a chunked `ptr::swap_nonoverlapping` and needs only a
    /// fixed-size buffer, where a `replace` returns a whole `T` through the
    /// caller's frame.
    fn swap_contents(&self, other: &mut Self::Value);
}

impl<T> PristineCell for Mutex<T> {
    type Value = T;

    fn replace_contents(&self, value: T) -> T {
        core::mem::replace(&mut *self.lock(), value)
    }

    fn swap_contents(&self, other: &mut T) {
        core::mem::swap(&mut *self.lock(), other);
    }
}

impl<T> PristineCell for PreemptSpinMutex<T> {
    type Value = T;

    fn replace_contents(&self, value: T) -> T {
        core::mem::replace(&mut *self.lock(), value)
    }

    fn swap_contents(&self, other: &mut T) {
        core::mem::swap(&mut *self.lock(), other);
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

/// [`with_pristine`] for state too large to travel through a stack frame.
///
/// Takes the pristine value by `&mut` and swaps it into place rather than
/// moving it in, so no whole `C::Value` is ever materialised in a frame — see
/// this module's "Size" section for the case that forced it.  Behaviour is
/// otherwise identical, including the concurrency caveat on [`with_pristine`].
///
/// The caller owns `scratch` and is responsible for it being pristine on
/// entry.  At this size that means building it on the heap *element by
/// element*: `Box::new([const { X }; N])` constructs the array in the caller's
/// frame and only then copies it into the box, which defeats the entire point.
/// `crate::net::bridge::self_test` is the worked example.
///
/// On return `scratch` holds whatever the suite did to the substitute, and the
/// caller's `Box` frees it. Unlike [`with_pristine`] there is nothing reusable
/// left over, which is correct: a second run wants a second pristine table,
/// not this one again.
pub fn with_pristine_swapped<C: PristineCell, R>(
    state: &C,
    scratch: &mut C::Value,
    body: impl FnOnce() -> R,
) -> R {
    state.swap_contents(scratch);
    let result = body();
    state.swap_contents(scratch);
    result
}

/// A module-scope atomic, viewed as "a cell holding a `Copy` scalar".
///
/// The standard atomics share no trait, so this is the smallest one that lets
/// [`pristine_atomic`] be written once instead of once per width. `Relaxed` is
/// the only ordering offered because the only caller is a self-test wrapper
/// running before its suite: there is nothing to synchronise with.
pub trait AtomicScalar {
    /// The scalar the atomic holds.
    type Value: Copy;

    /// Read the current value.
    fn load_relaxed(&self) -> Self::Value;

    /// Overwrite the current value.
    fn store_relaxed(&self, value: Self::Value);
}

macro_rules! impl_atomic_scalar {
    ($($atomic:ty => $prim:ty),* $(,)?) => {
        $(
            impl AtomicScalar for $atomic {
                type Value = $prim;

                fn load_relaxed(&self) -> $prim {
                    self.load(core::sync::atomic::Ordering::Relaxed)
                }

                fn store_relaxed(&self, value: $prim) {
                    self.store(value, core::sync::atomic::Ordering::Relaxed);
                }
            }
        )*
    };
}

impl_atomic_scalar! {
    core::sync::atomic::AtomicBool => bool,
    core::sync::atomic::AtomicU8 => u8,
    core::sync::atomic::AtomicU16 => u16,
    core::sync::atomic::AtomicU32 => u32,
    core::sync::atomic::AtomicU64 => u64,
    core::sync::atomic::AtomicUsize => usize,
    core::sync::atomic::AtomicI32 => i32,
    core::sync::atomic::AtomicI64 => i64,
}

/// Restores the atomic it was made from when it goes out of scope.
///
/// Returned by [`pristine_atomic`]; there is no reason to name it other than
/// to bind it, and it must be bound — `let _ = pristine_atomic(..)` drops it
/// immediately and restores before the suite has run. Bind it as `let _name`.
pub struct RestoreOnDrop<'a, A: AtomicScalar> {
    cell: &'a A,
    saved: A::Value,
}

impl<A: AtomicScalar> Drop for RestoreOnDrop<'_, A> {
    fn drop(&mut self) {
        self.cell.store_relaxed(self.saved);
    }
}

/// Install `pristine` in `cell`, and put the previous value back when the
/// returned guard is dropped.
///
/// The companion to [`with_pristine`] for state that does not live in the
/// table: free-standing counters, and — more importantly — the flags that gate
/// a module's `init()`. See this module's header for why installing the
/// pristine value matters rather than merely restoring the old one afterwards.
///
/// A guard per atomic, rather than one call taking a list, so that each keeps
/// its own type: no widening to a common integer, and no `dyn`.
///
/// ```ignore
/// let _init = pristine_atomic(&INITIALIZED, false);
/// let _sent = pristine_atomic(&QUERIES_SENT, 0);
/// with_pristine(&STATE, MdnsState::new(), self_test_inner)
/// ```
pub fn pristine_atomic<A: AtomicScalar>(cell: &A, pristine: A::Value) -> RestoreOnDrop<'_, A> {
    let saved = cell.load_relaxed();
    cell.store_relaxed(pristine);
    RestoreOnDrop { cell, saved }
}

// ---------------------------------------------------------------------------
// Skips
// ---------------------------------------------------------------------------

/// The sections of a self-test that could not run, carried to the line that
/// announces the result.
///
/// ## The problem this exists for
///
/// A self-test's **last** line is the one a reader believes.  Nearly every
/// suite in this tree ends with one — `[pcid] Self-test PASSED`, `[svcstart]
/// All 11 self-tests passed.` — and several of them can skip a section first:
/// no filesystem is mounted, the CPU has no PCID, QEMU was started without an
/// AC97 device.  When the skip is announced mid-run and the closing line is
/// unconditional, a run that tested half of what it claims is
/// byte-indistinguishable in the log from one that tested all of it.  The
/// half-run is then believed, indefinitely: `kernel/src/fs/index.rs` had this
/// shape, and 26 of lane B's Path-Z rungs no-op'd unnoticed for weeks for the
/// same reason (`known-issues.md` → `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT`).
///
/// A skip that is reported gets acted on; a silent one gets believed.
///
/// ## What it does not fix
///
/// Reporting is only half of it, and the smaller half.  The *condition* that
/// decides to skip must be a question about the environment — is `/tmp` in the
/// mount table, does `CPUID` advertise the feature — and never a discarded
/// `Result` from the code under test.  `if mkdir(d).is_ok() { ..test.. } else
/// { skip }` reads as "skip when this filesystem has no directories" but means
/// "skip on **any** failure", so the worse the code under test gets, the more
/// sections switch themselves off.  `scripts/check-selftest-skips.py` refuses
/// both shapes; this type only helps with the second.
///
/// ## Using it
///
/// ```ignore
/// let mut skips = Skips::new();
/// if tmp_mounted() {
///     /* ..the section.. */
/// } else {
///     skips.record("VFS add/search", "/tmp not mounted");
/// }
/// skips.report("[index]");
/// serial_println!("[index] Self-test passed{}", skips.suffix());
/// ```
#[derive(Debug, Default)]
pub struct Skips {
    /// `(section, why)` pairs, in the order the sections were reached.
    ///
    /// `&'static str` rather than `String` on purpose: a skip reason is a
    /// property of the code, not of the run, so there is nothing to format and
    /// no allocation to fail in a suite that may be diagnosing the allocator.
    entries: alloc::vec::Vec<(&'static str, &'static str)>,
}

impl Skips {
    /// An empty record.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: alloc::vec::Vec::new(),
        }
    }

    /// Record that `section` did not run, because `why`.
    ///
    /// `why` should name the missing *precondition* ("no AC97 device",
    /// "/tmp not mounted"), not the failing call — the reader's next question
    /// is always whether the absence is expected on this machine.
    pub fn record(&mut self, section: &'static str, why: &'static str) {
        self.entries.push((section, why));
    }

    /// Whether every section ran.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many sections did not run.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Print one `SKIP:` line per skipped section, prefixed with `tag`.
    ///
    /// Call this immediately before the closing line, not at the point of the
    /// skip: the two lines being adjacent is what stops a reader who scrolls
    /// to the bottom from missing it.  Prints nothing when nothing was
    /// skipped.
    pub fn report(&self, tag: &str) {
        for (section, why) in &self.entries {
            crate::serial_println!("{}   SKIP: {} ({})", tag, section, why);
        }
    }

    /// Text to append to a suite's closing line: empty when every section ran,
    /// and a count of what did not when some did.
    ///
    /// Returning a suffix rather than printing the whole line keeps each
    /// suite's own wording — `Self-test PASSED`, `All 11 self-tests passed.`,
    /// `Self-test passed (148 entries, 1 rebuilds)` — which is what makes this
    /// a one-line change at ~25 call sites rather than a rewrite of each.
    #[must_use]
    pub fn suffix(&self) -> alloc::string::String {
        if self.entries.is_empty() {
            alloc::string::String::new()
        } else {
            alloc::format!(" — {} section(s) SKIPPED", self.entries.len())
        }
    }
}

/// The outcome of a self-test's setup step, split into the two things that
/// `.is_ok()` collapses into one.
///
/// `if mkdir(d).is_ok() { ..section.. } else { skip }` reads as "skip when
/// this filesystem has no directories", but what it means is "skip on **any**
/// failure" — a permission gate refusing the `mkdir`, a full disk, a bug in
/// the directory code itself.  The worse the code under test gets, the more
/// sections switch themselves off, and the suite goes green.  Splitting the
/// failure is what stops that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setup {
    /// The step succeeded; run the section.
    Ready,
    /// This system does not implement the operation.  A legitimate reason to
    /// skip the section — and to say so.
    Unsupported(crate::error::KernelError),
    /// The step failed for a reason that is not a missing feature.  That is a
    /// defect, and the section must fail rather than quietly not run.
    Failed(crate::error::KernelError),
}

/// Classify a setup step's result.
///
/// Only three errors mean "this system cannot do that":
/// - `NotSupported` — the filesystem or driver does not implement it,
/// - `ReadOnlyFilesystem` — it could, but this mount forbids writes,
/// - `NoSuchDevice` — the hardware the section drives is not present.
///
/// Everything else, `PermissionDenied` and `IoError` above all, describes a
/// system that was *asked* and *refused*, which is the answer a test exists to
/// notice.
#[must_use]
pub fn classify<T>(r: crate::error::KernelResult<T>) -> Setup {
    use crate::error::KernelError;
    match r {
        Ok(_) => Setup::Ready,
        Err(
            e @ (KernelError::NotSupported
            | KernelError::ReadOnlyFilesystem
            | KernelError::NoSuchDevice),
        ) => Setup::Unsupported(e),
        Err(e) => Setup::Failed(e),
    }
}

/// Run a section's setup steps, then say whether the section may run.
///
/// Evaluates each step in order and stops at the first failure:
/// - every step `Ok` → `true`, and the section runs;
/// - the first failure is an environment limit → the skip is recorded on
///   `$skips` and this evaluates to `false`;
/// - any other failure → prints it and **returns** `Err` from the enclosing
///   function, because a setup step that fails for a reason other than "this
///   system cannot do that" is exactly the defect the section exists to find.
///
/// The enclosing function must therefore return `KernelResult<_>`.
///
/// ```ignore
/// let mut skips = Skips::new();
/// let ready = selftest_setup!(
///     skips, "[fs::handle]", "no-follow chown", "no symlink/chown support",
///     Vfs::write_file(target, b"x"),
///     Vfs::symlink(link, target),
///     Vfs::set_owner(target, 1000, 1000),
/// );
/// if ready { /* ..the section.. */ }
/// ```
#[macro_export]
macro_rules! selftest_setup {
    ($skips:expr, $tag:expr, $section:expr, $why:expr, $($step:expr),+ $(,)?) => {{
        let mut ready = true;
        $(
            if ready {
                match $crate::fs::selftest::classify($step) {
                    $crate::fs::selftest::Setup::Ready => {}
                    $crate::fs::selftest::Setup::Unsupported(_) => {
                        $skips.record($section, $why);
                        ready = false;
                    }
                    $crate::fs::selftest::Setup::Failed(e) => {
                        $crate::serial_println!(
                            "{}   FAIL: setup for '{}' failed with {:?} — that is not a \
                             missing feature, so it is a defect rather than a reason to \
                             skip the section",
                            $tag,
                            $section,
                            e
                        );
                        return Err($crate::error::KernelError::InternalError);
                    }
                }
            }
        )+
        ready
    }};
}
