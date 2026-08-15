//! Storage for state that is per-*process* in POSIX but must not be shared
//! between host test threads.
//!
//! The companion to [`crate::perthread`], and easy to confuse with it — the
//! distinction is which scope is the *real* one:
//!
//! | | real scope | why the host build differs |
//! |---|---|---|
//! | [`crate::perthread`] | per-**thread** | it doesn't — the target is per-thread too |
//! | this module | per-**process** | libtest puts many "processes" in one process |
//!
//! ## The problem
//!
//! A posix process owns exactly one fd table, one rlimit table, one set of
//! signal dispositions, one process group.  Modelling those as `static mut`
//! is correct on the target, where the crate is linked into one process.
//!
//! It is wrong under `cargo test`.  libtest runs every `#[test]` on its own
//! thread inside a single process, so a `static mut` is shared by every test
//! running concurrently — and these are exactly the statics tests mutate.
//! The failures are rare, cross-module and look nothing like their cause: a
//! test asserting fd 3 is closed fails because an unrelated test in another
//! module happened to `open()` at the same moment.  Six such tests, in five
//! modules, were found by running the suite 40 times; see known-issues.md
//! `TD-POSIX-TEST-SHARED-STATICS-REMAINING-TIER`.
//!
//! ## The fix
//!
//! [`process_global!`] declares the storage once and picks its scope by
//! target: one `static mut` on the target, one `thread_local!` on the host.
//! A test thread then stands in for a process, which is what the tests
//! already assume they are.  Nothing about the target build changes.
//!
//! A consequence worth relying on: **per-thread storage is the reset.**
//! Every test starts from `$init`, so converted modules can drop their
//! `reset_*()` helpers and their test-only mutexes rather than porting
//! them.  This holds even under `--test-threads=1` — libtest spawns a
//! thread per test at *any* concurrency, so tests never share one
//! thread's copy (verified directly: a write in one test is invisible to
//! the next when the two are run serially).
//!
//! The rejected alternatives — a test-only `Mutex`, and running the suite
//! with `--test-threads=1` — are recorded in design-decisions.md §110.
//!
//! ## What does *not* belong here
//!
//! State that is genuinely indexed by thread (`pthread.rs`'s thread-specific
//! data table) or that is an exported C ABI global the caller can take the
//! address of (`getopt.rs`'s `optind`/`optarg`) must stay process-global on
//! both builds — moving those would change observable semantics rather than
//! just isolate tests.

/// Declare a per-process global as an accessor returning a raw pointer.
///
/// ```ignore
/// process_global! {
///     /// This process's fd table.
///     pub(crate) fn fd_table() -> FdTable = FDS_INIT;
/// }
/// ```
///
/// expands to a `*mut FdTable` accessor over a `static mut` on the target
/// and over a `thread_local!` on host builds.  `$init` must be a `const`
/// expression: it initialises a `static` on one arm and a `const`-initialised
/// `thread_local!` on the other, so the host arm costs no lazy-init check and
/// lives in `.tbss` rather than bloating the binary.
///
/// # Why a raw pointer and not `&mut`
///
/// Every call site here manipulates a fixed-size table in place and hands
/// out interior pointers; a raw pointer keeps the `unsafe` (and the aliasing
/// obligation) visible at those sites instead of laundering it through a
/// safe-looking accessor that returns a `&'static mut`.
///
/// # Size
///
/// The host arm places the value in thread-local storage, which the OS
/// allocates and zeroes at *every* thread creation — and libtest creates one
/// thread per test (20k+ in this crate).  Keep values here to a few KiB.  For
/// anything approaching a megabyte, hand-roll a lazily heap-allocated
/// `thread_local!` instead (see `fdtable.rs`'s 1 MiB path table).
macro_rules! process_global {
    ($(
        $(#[$attr:meta])*
        $vis:vis fn $name:ident() -> $ty:ty = $init:expr;
    )*) => {$(
        $(#[$attr])*
        #[cfg(target_os = "none")]
        #[inline]
        $vis fn $name() -> *mut $ty {
            static mut STORAGE: $ty = $init;
            // SAFETY: the target links this crate into one single-threaded
            // process, so this is the process's only view of the value.
            &raw mut STORAGE
        }

        $(#[$attr])*
        #[cfg(not(target_os = "none"))]
        #[inline]
        $vis fn $name() -> *mut $ty {
            std::thread_local! {
                static STORAGE: core::cell::UnsafeCell<$ty> =
                    const { core::cell::UnsafeCell::new($init) };
            }

            /// Reached only while this thread's TLS is being destroyed, when
            /// `STORAGE` is gone.  A libc call from a `Drop` impl must get
            /// somewhere harmless to write rather than panic.
            static mut FALLBACK: $ty = $init;

            // `try_with`, not `with`: `with` panics after TLS teardown.
            STORAGE
                .try_with(core::cell::UnsafeCell::get)
                .unwrap_or(&raw mut FALLBACK)
        }
    )*};
}

pub(crate) use process_global;

// ---------------------------------------------------------------------------
// Slot-pool serialisation
// ---------------------------------------------------------------------------

/// A spin lock serialising *scans* of one fixed-size slot table.
///
/// ## What it is for
///
/// Every fixed-size pool in this crate — `dirent`'s `DIR` slots, `epoll`'s
/// instances, `mqueue`'s descriptors, the three System V tables, … — used to
/// claim a slot like this:
///
/// ```ignore
/// for (i, slot) in table.iter_mut().enumerate() {
///     if !slot.in_use {       // check ...
///         slot.in_use = true; // ... then set, with nothing held
///         return Some(i);
///     }
/// }
/// ```
///
/// That is correct only in a single-threaded process.  This crate implements
/// `pthread_create`, so two threads calling `opendir` (or `epoll_create`, or
/// `mq_open`, …) at the same moment could both read `in_use == false` for the
/// same slot and both return it — after which each would silently scribble on
/// the other's state.  `PoolLock` closes that window: the whole scan-and-claim
/// runs inside one critical section.
///
/// ## Why a lock and not a per-slot atomic
///
/// A `compare_exchange` on a per-slot `in_use` flag would make the *simple*
/// pools safe, but not the *compound* ones.  `sysv_msg::alloc_queue(key)`,
/// `sysv_sem::alloc_set(key, …)` and `sysv_shm::alloc_segment(key, …)` first
/// scan for an existing entry with a matching key and only allocate if there
/// is none; an atomic per-slot claim would still let two threads each create
/// a segment for the same key, because the lookup and the claim have to be
/// one indivisible step, not two.  One primitive that covers both beats two
/// primitives where the weaker one silently does not apply.  See
/// design-decisions.md §301.
///
/// ## What it does *not* cover
///
/// Only operations that **scan or search the table** take the lock:
/// allocation, release, and find-by-key.  Once a caller holds the `DIR *`,
/// `mqd_t` or index that names its own slot, it uses that slot without the
/// lock — POSIX makes concurrent use of one such handle the *caller's*
/// problem, and taking a process-wide lock on every `readdir` would serialise
/// unrelated directory streams for nothing.  The lock is therefore on the
/// *pool*, not on the objects in it.
///
/// ## The lock's scope must match its table's scope
///
/// This crate's pools are declared two ways, and a lock is only a lock if it
/// is shared exactly as widely as the thing it guards:
///
/// | the table is | declare the lock | why |
/// |---|---|---|
/// | [`process_global!`] (`epoll`, `mqueue`, `semaphore`, `aio`) | `process_global!` | on the host each test thread owns its *own* table, so a shared lock would serialise threads that cannot collide |
/// | a plain `static mut` (`dirent`, `stdio`, the three System V tables) | a plain `static` | the table really is shared on both builds, so a per-thread lock would guard nothing |
///
/// Getting this backwards is silent: a too-narrow lock still compiles, still
/// passes every single-threaded test, and protects nothing.  Prefer the
/// `process_global!` form wherever the table allows it — a spin lock has no
/// poisoning and no unwinding path, so one host test that panics inside the
/// critical section of a *shared* lock hangs every later test that touches
/// the same pool.
pub(crate) struct PoolLock {
    /// `true` while some thread is inside the critical section.
    held: core::sync::atomic::AtomicBool,
}

impl PoolLock {
    /// An unlocked pool lock.
    ///
    /// A `const fn` rather than an associated `const`: an associated const
    /// holding an `AtomicBool` is `clippy::declare_interior_mutable_const`,
    /// because every *use* of such a const is a fresh copy — so a caller who
    /// wrote `POOL_LOCK` where they meant `&POOL_LOCK` would get a private,
    /// always-unlocked lock and no diagnostic.  A constructor cannot be
    /// misread that way.
    pub(crate) const fn new() -> Self {
        Self {
            held: core::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// Proof that the pool guarded by a [`PoolLock`] is this thread's to scan.
///
/// Releasing on `Drop` is what makes the early `return` out of the middle of
/// a claim loop — the shape every one of these pools is written in — correct
/// without a manual unlock on each exit path.
#[must_use = "the pool is unlocked as soon as the guard is dropped"]
pub(crate) struct PoolGuard<'a> {
    lock: &'a PoolLock,
}

impl Drop for PoolGuard<'_> {
    fn drop(&mut self) {
        self.lock
            .held
            .store(false, core::sync::atomic::Ordering::Release);
    }
}

/// Acquire the pool lock at `lock`, spinning until it is free.
///
/// Modelled on `pthread.rs`'s `ATFORK_LOCK`: `Acquire` on the successful
/// exchange and `Release` on the drop pair up so that everything the previous
/// holder wrote to the table — above all the `in_use` flags — is visible to
/// the next one.
///
/// Allocation is not a hot path (it happens once per `opendir`, per
/// `epoll_create`, per `mq_open`), the critical section is a bounded scan of
/// a small fixed-size array with no calls out, and the crate is `no_std` on
/// the target with no blocking primitive available — so spinning is the right
/// shape here, and it cannot deadlock against itself as long as nothing
/// inside the section re-enters the same pool.
///
/// # Safety
///
/// `lock` must point to a live, initialised [`PoolLock`] that outlives the
/// returned guard — in practice always the pointer from a [`process_global!`]
/// accessor, which is valid for the rest of the process (host: the thread).
pub(crate) unsafe fn lock_pool<'a>(lock: *mut PoolLock) -> PoolGuard<'a> {
    // SAFETY: the caller guarantees `lock` points to a live `PoolLock` that
    // outlives `'a`.  `PoolLock`'s only field is an atomic, so a shared
    // reference is all the critical section needs and aliasing it from
    // several threads is exactly the intent.
    let lock: &'a PoolLock = unsafe { &*lock };
    while lock
        .held
        .compare_exchange_weak(
            false,
            true,
            core::sync::atomic::Ordering::Acquire,
            core::sync::atomic::Ordering::Relaxed,
        )
        .is_err()
    {
        core::hint::spin_loop();
    }
    PoolGuard { lock }
}

#[cfg(test)]
mod tests {
    use super::{PoolLock, lock_pool};

    process_global! {
        /// A table only these tests touch.
        fn probe() -> [u32; 4] = [0; 4];
    }

    /// The property the macro exists for: one test thread's writes must be
    /// invisible to another's.  This is the whole failure mode.
    #[test]
    fn writes_are_not_shared_between_threads() {
        // SAFETY: `probe()` is this thread's storage and no other thread has
        // the pointer.
        unsafe {
            (*probe())[0] = 99;
        }
        let child_saw = std::thread::spawn(|| {
            // SAFETY: as above, for the child's own storage.
            unsafe { (*probe())[0] }
        })
        .join()
        .expect("child thread panicked");
        assert_eq!(child_saw, 0, "child observed the parent's write");
        // SAFETY: as above.
        assert_eq!(unsafe { (*probe())[0] }, 99);
    }

    /// A fresh thread starts from `$init`, which is what lets the converted
    /// modules drop their per-test `reset_*()` helpers: per-thread storage
    /// *is* the reset.
    #[test]
    fn a_fresh_thread_starts_from_the_initialiser() {
        let seen = std::thread::spawn(|| {
            // SAFETY: this thread's own storage.
            unsafe { *probe() }
        })
        .join()
        .expect("child thread panicked");
        assert_eq!(seen, [0; 4]);
    }

    /// The pointer must be stable within a thread, or interior pointers
    /// handed out by one call would dangle by the next.
    #[test]
    fn the_pointer_is_stable_within_a_thread() {
        assert_eq!(probe(), probe());
    }

    // -----------------------------------------------------------------------
    // PoolLock
    // -----------------------------------------------------------------------

    /// A stand-in for the crate's slot pools, shaped exactly like them: a
    /// fixed-size table of `in_use` flags claimed by a linear scan.
    ///
    /// Declared as a plain `static` rather than with [`process_global!`]
    /// precisely because this test needs the *shared* case that the target
    /// build has and the host build otherwise does not.
    struct SharedPool {
        lock: PoolLock,
        slots: core::cell::UnsafeCell<[bool; SHARED_POOL_SLOTS]>,
    }

    /// Every access to `slots` is made under `lock`; that is the invariant
    /// under test.
    // SAFETY: see the doc comment — `slots` is only ever touched by a thread
    // holding `lock`, so no two threads reference it at once.
    unsafe impl Sync for SharedPool {}

    const SHARED_POOL_SLOTS: usize = 64;
    const SHARED_POOL_THREADS: usize = 8;
    const CLAIMS_PER_THREAD: usize = SHARED_POOL_SLOTS / SHARED_POOL_THREADS;

    static SHARED_POOL: SharedPool = SharedPool {
        lock: PoolLock::new(),
        slots: core::cell::UnsafeCell::new([false; SHARED_POOL_SLOTS]),
    };

    /// The bug the lock exists to prevent: two threads reading `in_use ==
    /// false` for the same slot and both claiming it.
    ///
    /// Three details make this fail *reliably* without the lock rather than
    /// once in a thousand runs, and each was arrived at by deleting
    /// `lock_pool` and checking the assertion actually trips:
    ///
    /// - a [`std::sync::Barrier`], so the threads are inside the claim loop
    ///   at the same moment rather than finishing before the next starts;
    /// - `yield_now()` between the read of `in_use` and the write, widening
    ///   the window from a few instructions to a scheduling quantum;
    /// - **no shared `Mutex` inside the loop.**  The first version of this
    ///   test pushed each claim onto a `Mutex<Vec<_>>`, which serialised the
    ///   threads all by itself and so passed with the lock removed — a test
    ///   that proved nothing.  Each thread now accumulates privately and the
    ///   results are merged after the join.
    #[test]
    fn a_scan_and_claim_under_the_lock_never_hands_out_a_slot_twice() {
        let start = std::sync::Barrier::new(SHARED_POOL_THREADS);
        let mut claimed: Vec<usize> = Vec::with_capacity(SHARED_POOL_SLOTS);

        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..SHARED_POOL_THREADS)
                .map(|_| {
                    let start = &start;
                    scope.spawn(move || {
                        let mut mine = Vec::with_capacity(CLAIMS_PER_THREAD);
                        start.wait();
                        for _ in 0..CLAIMS_PER_THREAD {
                            // SAFETY: `SHARED_POOL` is a `static`, so its
                            // lock outlives the guard.
                            let _guard =
                                unsafe { lock_pool((&raw const SHARED_POOL.lock).cast_mut()) };
                            // SAFETY: the guard is held, and holding it is
                            // the only way any thread reaches `slots`.
                            let slots = unsafe { &mut *SHARED_POOL.slots.get() };
                            for (i, in_use) in slots.iter_mut().enumerate() {
                                if !*in_use {
                                    std::thread::yield_now();
                                    *in_use = true;
                                    mine.push(i);
                                    break;
                                }
                            }
                        }
                        mine
                    })
                })
                .collect();
            for handle in handles {
                claimed.extend(handle.join().expect("claiming thread panicked"));
            }
        });

        assert_eq!(claimed.len(), SHARED_POOL_SLOTS, "a claim found no slot");
        claimed.sort_unstable();
        claimed.dedup();
        assert_eq!(
            claimed.len(),
            SHARED_POOL_SLOTS,
            "a slot was handed out to two threads at once"
        );
    }

    /// The guard must release on drop, including on the early `return` out
    /// of the middle of a claim loop that every pool is written with.
    #[test]
    fn the_guard_releases_the_lock_when_it_is_dropped() {
        static LOCK: PoolLock = PoolLock::new();

        fn claim_and_return_early() -> usize {
            // SAFETY: `LOCK` is a `static`, so it outlives the guard.
            let _guard = unsafe { lock_pool(&raw const LOCK as *mut _) };
            for i in 0..4 {
                if i == 2 {
                    return i;
                }
            }
            usize::MAX
        }

        assert_eq!(claim_and_return_early(), 2);
        assert!(
            !LOCK.held.load(core::sync::atomic::Ordering::Relaxed),
            "returning from inside the critical section left the lock held"
        );
        // A second acquisition would spin forever if the first had leaked.
        assert_eq!(claim_and_return_early(), 2);
    }
}
