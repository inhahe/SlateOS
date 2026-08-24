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

/// How many skipped sections one suite can name individually.
///
/// The largest suite in the tree records six; sixteen leaves room without
/// making the type big enough to matter on a stack (16 × 16 B = 256 B).
/// Passing it is not silently lossy — see [`Skips::record`].
const MAX_SKIPS: usize = 16;

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
///
/// ## Why it is fixed-capacity and allocation-free
///
/// This type must not touch the heap, and the reason is not tidiness — it is a
/// boot-killing bug it caused on 2026-08-23.  Two of its users run where a
/// heap allocation is either impossible or self-defeating:
///
/// * **Before the heap exists.**  `mm::frame::self_test()` runs between
///   "physical frame allocator initialized" and "kernel heap allocator
///   initialized".  When the ledger held a `Vec`, its first `record()` was the
///   first `Vec::push` of the boot, and the kernel died with `memory
///   allocation of 128 bytes failed` — from the suite that exists to prove the
///   memory subsystem works.
/// * **While diagnosing the allocator.**  A suite hunting a heap bug must not
///   report its findings through the heap: the reporting path would fail
///   exactly when there is something to report.
///
/// So the entries are `(&'static str, &'static str)` in an inline array of
/// [`MAX_SKIPS`], and [`suffix`](Skips::suffix) returns a `Display` adaptor
/// rather than a `String`.  A skip reason is a property of the *code*, not of
/// the run, so there is never anything to format and never an allocation to
/// fail.  [`self_test`] asserts this against the allocator's own counters.
#[derive(Debug)]
pub struct Skips {
    /// `(section, why)` pairs, in the order the sections were reached.
    entries: [(&'static str, &'static str); MAX_SKIPS],
    /// How many of `entries` are live.
    len: usize,
    /// Sections that skipped after `entries` filled.  Counted rather than
    /// dropped: losing a skip is the exact failure this whole module exists to
    /// prevent, so overflow degrades to "N more, unnamed" and never to silence.
    overflow: usize,
}

impl Default for Skips {
    fn default() -> Self {
        Self::new()
    }
}

impl Skips {
    /// An empty record.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: [("", ""); MAX_SKIPS],
            len: 0,
            overflow: 0,
        }
    }

    /// Record that `section` did not run, because `why`.
    ///
    /// `why` should name the missing *precondition* ("no AC97 device",
    /// "/tmp not mounted"), not the failing call — the reader's next question
    /// is always whether the absence is expected on this machine.
    ///
    /// Beyond [`MAX_SKIPS`] the section is counted but not named; the count
    /// still reaches both [`report`](Self::report) and
    /// [`suffix`](Self::suffix), so the closing line stays honest.
    pub fn record(&mut self, section: &'static str, why: &'static str) {
        if let Some(slot) = self.entries.get_mut(self.len) {
            *slot = (section, why);
            self.len = self.len.saturating_add(1);
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    /// How many sections were skipped, named and unnamed.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.len.saturating_add(self.overflow)
    }

    /// Whether every section ran.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// Print one `SKIP:` line per skipped section, prefixed with `tag`.
    ///
    /// Call this immediately before the closing line, not at the point of the
    /// skip: the two lines being adjacent is what stops a reader who scrolls
    /// to the bottom from missing it.  Prints nothing when nothing was
    /// skipped.
    pub fn report(&self, tag: &str) {
        // `get(..len)` rather than indexing: `indexing_slicing` is denied, and
        // an out-of-range `len` would be a bug in this type, not a reason to
        // panic a self-test.
        for (section, why) in self.entries.get(..self.len).unwrap_or(&[]) {
            crate::serial_println!("{}   SKIP: {} ({})", tag, section, why);
        }
        if self.overflow > 0 {
            crate::serial_println!(
                "{}   SKIP: {} further section(s) (ledger holds {})",
                tag,
                self.overflow,
                MAX_SKIPS
            );
        }
    }

    /// Text to append to a suite's closing line: empty when every section ran,
    /// and a count of what did not when some did.
    ///
    /// Returning a suffix rather than printing the whole line keeps each
    /// suite's own wording — `Self-test PASSED`, `All 11 self-tests passed.`,
    /// `Self-test passed (148 entries, 1 rebuilds)` — which is what makes this
    /// a one-line change at ~25 call sites rather than a rewrite of each.
    ///
    /// The returned value formats in place with `{}`, so it allocates nothing;
    /// see the type-level note on why that matters.
    #[must_use]
    pub const fn suffix(&self) -> SkipSuffix {
        SkipSuffix(self.count())
    }
}

/// The `{}`-formattable tail of a self-test's closing line.
///
/// Exists so [`Skips::suffix`] can be used exactly like the `String` it
/// replaced — `serial_println!("… PASSED{}", skips.suffix())` — without an
/// allocation on a path that may have no working allocator.
#[derive(Debug, Clone, Copy)]
pub struct SkipSuffix(usize);

impl core::fmt::Display for SkipSuffix {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.0 == 0 {
            Ok(())
        } else {
            write!(f, " — {} section(s) SKIPPED", self.0)
        }
    }
}

/// Whether `path` is a mount point in the live mount table.
///
/// This is the shape a filesystem precondition should take: a fact looked up,
/// with exactly one meaning.  The alternatives a suite reaches for instead —
/// `Vfs::stat(path).is_ok()`, or a probe write whose `Result` is discarded —
/// read as "is it mounted" but mean "did that call fail for *any* reason",
/// which includes a permission gate wrongly refusing the path and the bug the
/// section exists to catch.
///
/// It answers only the question it names.  A mount can be present and still
/// refuse a write (read-only, quota, a file tag), so a section that needs to
/// *write* should use this to decide whether to run and then classify the
/// staging error with [`classify`] — see `fs::trash::self_test`.
#[must_use]
pub fn is_mounted(path: &str) -> bool {
    crate::fs::Vfs::mounts()
        .iter()
        .any(|(p, _)| p.as_path() == crate::fs::path::Path::new(path))
}

/// Whether `path` is a mount point *and* is not mounted read-only.
///
/// The read-only flag is the one write-blocking condition the mount table can
/// answer on its own; the rest (quotas, tags, ACLs) still need [`classify`].
#[must_use]
pub fn is_mounted_rw(path: &str) -> bool {
    crate::fs::Vfs::mounts_full()
        .iter()
        .any(|(p, _, opts)| p.as_path() == crate::fs::path::Path::new(path) && !opts.read_only)
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

/// A `core::fmt::Write` sink over a fixed stack buffer.
///
/// Used by the self-test below to render a [`SkipSuffix`] and inspect the
/// bytes.  It exists because the obvious alternative — `alloc::format!` —
/// would allocate, and the whole point of the assertion is that nothing on
/// this path does.
struct FixedBuf {
    bytes: [u8; 64],
    len: usize,
}

impl FixedBuf {
    const fn new() -> Self {
        Self {
            bytes: [0; 64],
            len: 0,
        }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(self.bytes.get(..self.len).unwrap_or(&[])).unwrap_or("<not utf-8>")
    }
}

impl core::fmt::Write for FixedBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &b in s.as_bytes() {
            let Some(slot) = self.bytes.get_mut(self.len) else {
                return Err(core::fmt::Error);
            };
            *slot = b;
            self.len = self.len.saturating_add(1);
        }
        Ok(())
    }
}

/// Prove that the skip ledger itself never touches the heap.
///
/// This is a regression test for a boot-killing bug, not a formality.  On
/// 2026-08-23 [`Skips`] held an `alloc::vec::Vec` and [`Skips::suffix`]
/// returned an `alloc::string::String`.  `mm::frame::self_test()` runs
/// *between* "physical frame allocator initialized" and "kernel heap allocator
/// initialized", so its first `skips.record(…)` was the first heap allocation
/// of the boot and the kernel died with `memory allocation of 128 bytes
/// failed` — killed by the bookkeeping of the suite that exists to prove the
/// memory subsystem works.
///
/// The invariant is structural (an inline array plus a `Display` adaptor), but
/// structure is easy to regress by adding one convenient field, and the
/// failure mode is a panic 120 lines into the boot with no mention of this
/// module in it.  So it is asserted against the allocator's own counters.
///
/// Interrupts are masked across the measurement because [`crate::mm::heap::stats`]
/// aggregates per-CPU counters: an interrupt handler that allocated inside the
/// window would be attributed to this code and fail the test spuriously.
///
/// # Errors
/// Returns [`KernelError::InternalError`](crate::error::KernelError::InternalError)
/// if the ledger allocated, dropped a skip, or rendered the wrong suffix.
pub fn self_test() -> crate::error::KernelResult<()> {
    use core::fmt::Write as _;

    crate::serial_println!("[selftest] Running skip-ledger self-test...");

    let mut rendered = FixedBuf::new();
    let mut overflowed = FixedBuf::new();
    let mut empty = FixedBuf::new();

    // The whole exercise runs inside the masked window, so the counters
    // bracket exactly the work under test and nothing else.
    let (before, after, over_count, named, emptiness) = crate::cpu::without_interrupts(|| {
        let b = crate::mm::heap::stats();

        let fresh = Skips::new();
        let _ = write!(empty, "{}", fresh.suffix());
        let fresh_is_empty = fresh.is_empty();

        let mut skips = Skips::new();
        // One past capacity, so the overflow arm is exercised too.  Every
        // string here is `'static`, which is what makes recording free.
        for _ in 0..MAX_SKIPS {
            skips.record("ledger capacity", "exercising the named path");
        }
        skips.record("ledger overflow", "exercising the counted path");
        let n = skips.count();
        let named = skips.entries.get(..skips.len).unwrap_or(&[]).len();
        let _ = write!(overflowed, "{}", skips.suffix());

        let mut one = Skips::new();
        one.record("suffix rendering", "exercising the singular path");
        let _ = write!(rendered, "{}", one.suffix());
        // `report` writes to the serial port; include it, because a
        // reporting path that allocates fails exactly when there is
        // something to report.
        one.report("[selftest]");

        let a = crate::mm::heap::stats();
        (b, a, n, named, (fresh_is_empty, skips.is_empty()))
    });

    let allocs = after
        .slab_allocs
        .saturating_sub(before.slab_allocs)
        .saturating_add(after.large_allocs.saturating_sub(before.large_allocs));
    if allocs != 0 {
        crate::serial_println!(
            "[selftest]   FAIL: the skip ledger performed {allocs} heap allocation(s); it runs \
             before the heap exists (mm::frame::self_test) and must not"
        );
        return Err(crate::error::KernelError::InternalError);
    }

    if !empty.as_str().is_empty() {
        crate::serial_println!(
            "[selftest]   FAIL: an empty ledger rendered {:?}, want \"\"",
            empty.as_str()
        );
        return Err(crate::error::KernelError::InternalError);
    }
    // `is_empty` is what a suite branches on to decide whether to print a
    // qualified closing line, so a ledger that under-reports emptiness would
    // reinstate the unqualified "PASSED" this module exists to prevent.
    if emptiness != (true, false) {
        crate::serial_println!(
            "[selftest]   FAIL: is_empty() reported {:?} for (fresh, overflowed), want (true, false)",
            emptiness
        );
        return Err(crate::error::KernelError::InternalError);
    }
    if rendered.as_str() != " — 1 section(s) SKIPPED" {
        crate::serial_println!(
            "[selftest]   FAIL: one skip rendered {:?}",
            rendered.as_str()
        );
        return Err(crate::error::KernelError::InternalError);
    }
    // The overflowed skip must still be *counted*: dropping it would restore
    // exactly the silence this module exists to prevent.
    if over_count != MAX_SKIPS.saturating_add(1) || named != MAX_SKIPS {
        crate::serial_println!(
            "[selftest]   FAIL: {} recorded past capacity {MAX_SKIPS} counted {over_count} \
             ({named} named)",
            MAX_SKIPS.saturating_add(1)
        );
        return Err(crate::error::KernelError::InternalError);
    }
    if overflowed.as_str() != " — 17 section(s) SKIPPED" {
        crate::serial_println!(
            "[selftest]   FAIL: an overflowed ledger rendered {:?}",
            overflowed.as_str()
        );
        return Err(crate::error::KernelError::InternalError);
    }

    crate::serial_println!("[selftest]   Ledger allocates nothing: OK");
    crate::serial_println!("[selftest]   Suffix rendering (0/1/overflow): OK");
    crate::serial_println!("[selftest]   Overflow is counted, not dropped: OK");
    crate::serial_println!("[selftest] Skip-ledger self-test PASSED");
    Ok(())
}
