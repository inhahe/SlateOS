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

#[cfg(test)]
mod tests {
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
}
