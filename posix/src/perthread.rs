//! Per-thread storage for the libc functions that POSIX defines as
//! returning a pointer into a static buffer.
//!
//! ## Why this exists
//!
//! A family of standard functions — `strerror`, `gmtime`, `localtime`,
//! `asctime`, `ctime`, `inet_ntoa`, `ether_aton`, `gethostbyname`,
//! `getservbyname`, `getprotobyname`, … — return `*mut T` pointing at storage the *library*
//! owns, and `errno` is likewise a single lvalue per thread.  POSIX permits
//! that storage to be overwritten by the next call **on the same thread**;
//! that is exactly why the `_r` reentrant variants exist.  Implemented with
//! a process-wide `static mut`, though, two threads calling the same
//! function race: one reads the buffer after the other has overwritten it.
//!
//! glibc and musl both make this storage *per-thread*, so the non-`_r`
//! functions are thread-safe in the only sense callers can rely on.  This
//! module provides that storage.
//!
//! It also holds the handful of values POSIX defines as *attributes of a
//! thread* rather than as return buffers — currently the cancellation
//! state and type.  The test is the same one that governs everything
//! here: if the spec says "the calling thread", the value belongs in this
//! block.  Contrast [`crate::perprocess`], which is for state that really
//! is per-process and only needs splitting so that host test threads can
//! stand in for processes.
//!
//! ## How it is stored
//!
//! Everything lives in one [`PerThread`] struct, and the struct lives at a
//! fixed offset from the thread pointer: immediately above the TCB, inside
//! the same mapping [`crate::tls`] already reserves for a thread's TLS
//! block (see [`crate::tls::TlsImage::reserve`]).  That choice has three
//! consequences worth stating:
//!
//! - **No allocation.**  Finding the block is one `%fs`-relative load and a
//!   constant offset; there is no lazy `malloc`, so no failure path and no
//!   re-entrancy hazard from calling the allocator inside `strerror`.
//! - **No teardown.**  The block is part of the thread's stack+TLS mapping,
//!   which `pthread_join`/`pthread_detach` already unmap.  There is no
//!   second lifetime to get wrong.
//! - **Zero-initialised for free.**  The mapping is fresh anonymous memory,
//!   and every field's correct initial state is all-zero (`errno == 0`,
//!   empty buffers, null result pointers).  [`PerThread::ZERO`] is the same
//!   value, used for the host build and the fallback below.
//!
//! ## The no-thread-pointer fallback
//!
//! A thread only has a `%fs` base once [`crate::tls`] installed one, so
//! before `__libc_start_main` runs — or in a bare-metal `services/` binary
//! that links this crate without the crt — reading `%fs:0` would fault.
//! [`current`] therefore returns a process-global fallback block when no
//! thread pointer is installed.  That is precisely the old behaviour (one
//! shared buffer), so such programs are no worse off than before; they are
//! also single-threaded by construction, since `pthread_create` cannot run
//! without the crt.
//!
//! ## Host builds
//!
//! On the host (unit tests) there is no `%fs` and no TLS block, so the
//! block is a `std::thread_local!` instead.  This is not merely a stub: the
//! test harness runs tests on many threads in one process, and a shared
//! buffer made a rotating handful of them fail per run (known-issues.md
//! `TD-POSIX-TEST-PARALLEL`).

use core::mem::{align_of, size_of};

/// All per-thread libc storage, in one block.
///
/// Every field must be valid when all-zero — see the module docs.  Keep it
/// `repr(C)` so the layout is stable and inspectable from a debugger at
/// `TP + TCB_SIZE`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PerThread {
    /// The thread's `errno`.  `__errno_location()` returns `&mut` this.
    ///
    /// Kept first so that `current().cast::<i32>()` is the errno pointer —
    /// `__errno_location` is on the hot path of every failing syscall.
    pub errno: i32,

    /// The thread's `h_errno` — the resolver error set by `gethostbyname` and
    /// friends.  `__h_errno_location()` returns `&mut` this.
    ///
    /// Separate from `errno` because the legacy resolver API reports through
    /// its own variable (`HOST_NOT_FOUND`/`TRY_AGAIN`/`NO_RECOVERY`/`NO_DATA`,
    /// which overlap the `errno` numbering and mean something else entirely).
    pub h_errno: i32,

    /// Result buffer for `gmtime`/`localtime`.
    ///
    /// All-zero is not a *meaningful* `struct tm` (`tm_mday == 0` is out of
    /// range), but it is the same thing the old process-wide static held
    /// before the first call, and every path that returns a pointer to it
    /// fully overwrites it first.
    pub tm: crate::time::Tm,

    /// Result buffer for `getdate`: its own, as glibc's is, so a
    /// `localtime` between a `getdate` and its use does not overwrite it.
    pub getdate: crate::time::Tm,

    /// Result buffer for `asctime`/`ctime`.
    ///
    /// Big enough for every `struct tm`, as glibc's is: `asctime` formats
    /// any year and any field width, and only `asctime_r`'s 26 bytes are a
    /// limit (see [`crate::time::ASCTIME_MAX`]).
    pub asctime: [u8; crate::time::ASCTIME_MAX],

    /// Result buffer for `inet_ntoa`.  Exactly fits `"255.255.255.255\0"`.
    pub inet_ntoa: [u8; 16],

    /// Result buffer for `ether_aton`.
    pub ether_aton: crate::inet::EtherAddr,

    /// Result buffer for `ether_ntoa`: `"xx:xx:xx:xx:xx:xx\0"`.
    pub ether_ntoa: [u8; 18],

    /// This thread's netdb state ([`crate::netdb`]'s `ThreadDb`): the
    /// blocks `getservbyname` and its kin answer in, and each database's
    /// enumeration.  NULL until the first call; freed as the thread exits.
    pub netdb: *mut u8,

    /// The thread's cancellation state — `PTHREAD_CANCEL_ENABLE` (0) or
    /// `PTHREAD_CANCEL_DISABLE`.
    ///
    /// Unlike the buffers above, this is not a "returns a pointer to
    /// static storage" case: POSIX makes cancellability a property *of
    /// the thread*, and `pthread_setcancelstate` is defined to change it
    /// for the calling thread only.  It lived in a process-global atomic
    /// until it was found sharing state between test threads, which is
    /// the same bug the target build would have had the moment a program
    /// called `pthread_create`.
    ///
    /// Zero is `PTHREAD_CANCEL_ENABLE`, which is POSIX's required initial
    /// value for a new thread — so the all-zero invariant holds.
    pub cancel_state: i32,

    /// The thread's cancellation type — `PTHREAD_CANCEL_DEFERRED` (0) or
    /// `PTHREAD_CANCEL_ASYNCHRONOUS`.  Per-thread for the same reason as
    /// [`Self::cancel_state`], and zero is likewise POSIX's required
    /// initial value.
    pub cancel_type: i32,

    /// The thread's `arc4random` ChaCha20 pool.
    ///
    /// Per-thread so `arc4random` needs no lock and two threads can never be
    /// handed the same bytes.  All-zero means "never seeded", which is what
    /// this block starts as — see [`crate::random::RandomState`].
    pub random: crate::random::RandomState,
    /// The thread's kernel task id, or 0 before the first
    /// [`crate::pthread::current_tid`] fetched it.  Cached because every
    /// mutex lock records its owner, and a syscall there was the uncontended
    /// path's whole cost.  `fork`'s child resets it: its id is new.
    pub tid: i32,

    /// Address of this thread's slot in `pthread`'s thread table, or 0 for
    /// the initial thread.  `pthread_create` writes it into the new thread's
    /// block before the thread starts, so the thread can reach its slot
    /// without looking itself up by an id its creator may not have published
    /// yet.
    pub thread_slot: usize,

    /// This thread's thread-specific-data values: [`TSD_BLOCKS`] blocks of
    /// `pthread`'s entries, each allocated (zeroed) when the thread first
    /// sets a key in it and freed when it exits.
    pub tsd: [*mut u8; TSD_BLOCKS],

    /// A value was set since the last destructor sweep -- glibc's
    /// `specific_used`, which decides whether a sweep repeats.
    pub tsd_used: bool,

    /// This thread's `thread_local` destructors, newest first: NULL, or the
    /// head of a list of [`crate::exit_list`]'s nodes, each `malloc`ed by
    /// `__cxa_thread_atexit_impl` and freed as it runs.  Run when the thread
    /// ends and, for the thread calling it, by `exit` -- glibc's
    /// `tls_dtor_list`.
    pub tls_dtors: *mut u8,

    /// This thread's cleanup handlers, innermost first: NULL, or the newest
    /// of the `struct __ptcb`s that musl's `pthread_cleanup_push` macro
    /// declares on the pushing function's own stack
    /// ([`crate::pthread::Ptcb`]). `pthread_exit` runs what is left of it.
    pub cleanup: *mut crate::pthread::Ptcb,

    /// The text [`crate::string::strerror`] gives a number that is no
    /// error's: glibc's "Unknown error N", right only for the thread that
    /// asked. "Unknown error -2147483648" and its NUL are 26 bytes.
    pub strerror: [u8; 32],

    /// The same for [`crate::signal::strsignal`]: "Unknown signal N" and
    /// "Real-time signal N".
    pub strsignal: [u8; 32],

    /// The `X` [`crate::prng::seed48`] replaced, which it returns a pointer
    /// to: the calling thread's own, so that another thread's `seed48` cannot
    /// overwrite it while it is read.
    pub seed48: [u16; 3],

    /// The restartable multibyte conversions' internal states, for a NULL
    /// `ps`: one a function, as C requires, and the thread's own
    /// ([`crate::wchar::internal`]).
    pub mbstate: [crate::wchar::MbstateT; crate::wchar::internal::COUNT],
}

/// `prefix` and then `n` in decimal, NUL-terminated, into one of the
/// block's message buffers ([`PerThread::strerror`], [`PerThread::strsignal`]);
/// the start of it. Every prefix used is short enough for any `i32` to fit
/// after it; a longer one would be cut short, never let overrun.
pub(crate) fn numbered(buf: &mut [u8; 32], prefix: &str, n: i32) -> *const u8 {
    struct Cursor<'a> {
        buf: &'a mut [u8; 32],
        len: usize,
    }
    impl core::fmt::Write for Cursor<'_> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let end = self.len.checked_add(s.len()).ok_or(core::fmt::Error)?;
            // The last byte is the NUL's.
            if end >= self.buf.len() {
                return Err(core::fmt::Error);
            }
            let dst = self.buf.get_mut(self.len..end).ok_or(core::fmt::Error)?;
            dst.copy_from_slice(s.as_bytes());
            self.len = end;
            Ok(())
        }
    }
    let mut c = Cursor { buf, len: 0 };
    // Cut short, as above, is the only way this fails.
    let _ = core::fmt::write(&mut c, format_args!("{prefix}{n}"));
    let len = c.len;
    if let Some(b) = c.buf.get_mut(len) {
        *b = 0;
    }
    c.buf.as_ptr()
}

/// Blocks of thread-specific data a thread can have: with `pthread`'s 32
/// values per block, 128 keys -- musl's `PTHREAD_KEYS_MAX`.
pub const TSD_BLOCKS: usize = 4;

impl PerThread {
    /// The initial state of a fresh thread's block.
    ///
    /// Must be bit-identical to all-zero: a thread's block is carved out of
    /// fresh anonymous memory and is never explicitly initialised.
    pub const ZERO: Self = Self {
        errno: 0,
        h_errno: 0,
        tm: crate::time::Tm::ZERO,
        getdate: crate::time::Tm::ZERO,
        asctime: [0; crate::time::ASCTIME_MAX],
        inet_ntoa: [0; 16],
        ether_aton: crate::inet::EtherAddr::ZERO,
        ether_ntoa: [0; 18],
        netdb: core::ptr::null_mut(),
        cancel_state: 0,
        cancel_type: 0,
        random: crate::random::RandomState::ZERO,
        tid: 0,
        thread_slot: 0,
        tsd: [core::ptr::null_mut(); TSD_BLOCKS],
        tsd_used: false,
        tls_dtors: core::ptr::null_mut(),
        cleanup: core::ptr::null_mut(),
        strerror: [0; 32],
        strsignal: [0; 32],
        seed48: [0; 3],
        mbstate: [crate::wchar::MbstateT::new(); crate::wchar::internal::COUNT],
    };
}

/// Bytes reserved for the per-thread block, rounded up so that placing it
/// at `TP + TCB_SIZE` (both multiples of 16) keeps the next thing aligned.
///
/// `crate::tls::TlsImage::reserve` adds this to every thread's mapping.
pub(crate) const BLOCK_SIZE: u64 = (size_of::<PerThread>() as u64).next_multiple_of(16);

/// The block sits at `TP + TCB_SIZE`, and `TP` is only guaranteed
/// 16-byte-aligned, so the struct may not need more than that.
const _: () = assert!(align_of::<PerThread>() <= 16);

/// Fallback block for threads with no thread pointer (see the module docs).
///
/// Deliberately a single shared instance: a program in this state has no
/// working `pthread_create`, so there is exactly one thread using it.
#[cfg(target_os = "none")]
static mut FALLBACK: PerThread = PerThread::ZERO;

#[cfg(not(target_os = "none"))]
std::thread_local! {
    /// Host stand-in for the TLS block.  `UnsafeCell` because callers want
    /// a raw `*mut` (that is the shape of the C ABI being emulated:
    /// `__errno_location` hands out a pointer the caller writes through).
    static HOST_BLOCK: core::cell::UnsafeCell<PerThread> =
        const { core::cell::UnsafeCell::new(PerThread::ZERO) };
}

/// Host fallback used only while a thread's TLS is being destroyed, when
/// `HOST_BLOCK` is no longer accessible.  Never reached in practice; see
/// [`current`].
#[cfg(not(target_os = "none"))]
static mut HOST_FALLBACK: PerThread = PerThread::ZERO;

/// Pointer to the calling thread's block.
///
/// Never null, and valid until the thread exits.  The returned pointer must
/// not be shared with another thread — that is the whole point of this
/// module, and it matches what POSIX says about the buffers these functions
/// return.
#[cfg(target_os = "none")]
#[must_use]
pub fn current() -> *mut PerThread {
    let tp = crate::tls::thread_pointer();
    if tp == 0 {
        // No thread pointer installed: reading %fs would fault.  See the
        // module docs — this is the pre-crt / bare-metal-service case.
        return &raw mut FALLBACK;
    }
    block_at(tp)
}

/// The block of the thread whose thread pointer is `tp`: immediately above
/// the TCB, where `TlsImage::reserve`/`thread_pointer` placed it.
///
/// `pthread_create` uses it to write into a new thread's block before the
/// thread runs.
#[must_use]
pub fn block_at(tp: u64) -> *mut PerThread {
    tp.wrapping_add(crate::tls::TCB_SIZE) as *mut PerThread
}

/// Host build: a `thread_local!` stands in for the TLS block.
#[cfg(not(target_os = "none"))]
#[must_use]
pub fn current() -> *mut PerThread {
    // `try_with` rather than `with`: `with` panics if the thread's TLS has
    // already been destroyed, which can happen if a `Drop` impl running
    // during thread teardown calls into libc.  Returning the shared
    // fallback there is strictly better than panicking, and by then the
    // thread is the only one that could still be using it.
    HOST_BLOCK
        .try_with(core::cell::UnsafeCell::get)
        .unwrap_or(&raw mut HOST_FALLBACK)
}

#[cfg(test)]
mod tests {
    use super::{BLOCK_SIZE, PerThread, current};

    #[test]
    fn block_size_is_a_multiple_of_sixteen() {
        assert_eq!(BLOCK_SIZE % 16, 0);
        assert!(BLOCK_SIZE >= core::mem::size_of::<PerThread>() as u64);
    }

    /// Every thread's stack+TLS mapping pays for this block, so it has to
    /// stay small.  If a new buffer pushes past the budget, weigh it against
    /// making that one buffer lazily allocated instead of raising this.
    #[test]
    fn the_block_stays_small_enough_to_ride_in_every_thread() {
        assert!(
            BLOCK_SIZE <= 2048,
            "per-thread block grew to {BLOCK_SIZE} bytes"
        );
    }

    /// The load-bearing invariant of this module: a thread's block is fresh
    /// anonymous memory that is never explicitly initialised, so all-zero
    /// must be a *valid* `PerThread` and must agree with [`PerThread::ZERO`].
    ///
    /// `mem::zeroed` also earns its keep at compile time — rustc's
    /// `invalid_value` lint rejects it outright if anyone ever adds a field
    /// whose zero bit pattern is invalid (a reference, a `NonNull`, an enum
    /// with no zero discriminant), which is exactly the mistake that would
    /// otherwise be silent UB on every newly-created thread.
    #[test]
    fn a_zero_filled_block_is_valid_and_matches_zero() {
        // SAFETY: every field is an integer, a byte array, a raw pointer, or
        // a `repr(C)` struct of those, so all-zero is a valid value.
        let zeroed: PerThread = unsafe { core::mem::zeroed() };
        assert_eq!(zeroed.errno, PerThread::ZERO.errno);
        assert_eq!(zeroed.asctime, PerThread::ZERO.asctime);
        assert_eq!(zeroed.inet_ntoa, PerThread::ZERO.inet_ntoa);
        assert_eq!(zeroed.ether_aton, PerThread::ZERO.ether_aton);
        assert_eq!(zeroed.ether_ntoa, PerThread::ZERO.ether_ntoa);
        assert_eq!(zeroed.netdb, PerThread::ZERO.netdb);
        assert_eq!(zeroed.tm.tm_sec, PerThread::ZERO.tm.tm_sec);
        assert_eq!(zeroed.tm.tm_year, PerThread::ZERO.tm.tm_year);
        assert_eq!(zeroed.tm.tm_isdst, PerThread::ZERO.tm.tm_isdst);
        assert_eq!(zeroed.getdate.tm_year, PerThread::ZERO.getdate.tm_year);
        assert!(zeroed.getdate.tm_zone.is_null());
        assert_eq!(zeroed.cancel_state, PerThread::ZERO.cancel_state);
        assert_eq!(zeroed.cancel_type, PerThread::ZERO.cancel_type);
        assert_eq!(zeroed.thread_slot, PerThread::ZERO.thread_slot);
        assert_eq!(zeroed.tsd, PerThread::ZERO.tsd);
        assert_eq!(zeroed.tsd_used, PerThread::ZERO.tsd_used);
        assert_eq!(zeroed.tls_dtors, PerThread::ZERO.tls_dtors);
        assert_eq!(zeroed.cleanup, PerThread::ZERO.cleanup);
    }

    /// A fresh thread must start cancellable and deferred, which POSIX
    /// requires and which the all-zero block is relied on to provide.
    #[test]
    fn a_fresh_thread_starts_from_the_posix_cancel_defaults() {
        let (state, ty) = std::thread::spawn(|| {
            (
                crate::pthread::current_cancel_state(),
                crate::pthread::current_cancel_type(),
            )
        })
        .join()
        .expect("child thread panicked");
        assert_eq!(state, crate::pthread::PTHREAD_CANCEL_ENABLE);
        assert_eq!(ty, crate::pthread::PTHREAD_CANCEL_DEFERRED);
    }

    /// The bug that moved this state here: one thread's
    /// `pthread_setcanceltype` must be invisible to another.
    #[test]
    fn cancel_type_is_not_shared_between_threads() {
        assert_eq!(
            crate::pthread::pthread_setcanceltype(
                crate::pthread::PTHREAD_CANCEL_ASYNCHRONOUS,
                core::ptr::null_mut(),
            ),
            0,
        );
        let child_saw = std::thread::spawn(crate::pthread::current_cancel_type)
            .join()
            .expect("child thread panicked");
        assert_eq!(
            child_saw,
            crate::pthread::PTHREAD_CANCEL_DEFERRED,
            "child observed the parent's cancel type",
        );
    }

    #[test]
    fn current_is_stable_within_a_thread() {
        assert_eq!(current(), current());
    }

    /// The property the whole module exists for: two threads must not see
    /// each other's block.
    #[test]
    fn each_thread_gets_its_own_block() {
        let mine = current() as usize;
        let theirs = std::thread::spawn(|| current() as usize)
            .join()
            .expect("child thread panicked");
        assert_ne!(mine, theirs);
    }

    /// A write through the pointer must be visible to a later read on the
    /// same thread, and invisible to another thread.
    #[test]
    fn writes_are_per_thread() {
        // SAFETY: `current()` is valid for this thread and no other thread
        // holds this pointer.
        unsafe {
            (*current()).errno = 4242;
        }
        let child_saw = std::thread::spawn(|| {
            // SAFETY: as above, for the child's own block.
            unsafe { (*current()).errno }
        })
        .join()
        .expect("child thread panicked");
        assert_eq!(child_saw, 0, "child saw the parent's errno");
        // SAFETY: as above.
        assert_eq!(unsafe { (*current()).errno }, 4242);
        // SAFETY: as above — leave the thread's errno as we found it.
        unsafe {
            (*current()).errno = 0;
        }
    }

    /// End-to-end version of the above, through the actual libc entry points
    /// rather than the raw block: this is the race that made
    /// `cargo test -p posix` flaky under the parallel harness
    /// (`known-issues.md` TD-POSIX-TEST-PARALLEL).  Two threads hammering
    /// `getprotobynumber` with different arguments must each keep reading
    /// their own answer.
    #[test]
    fn netdb_results_are_not_shared_between_threads() {
        /// Look up `number` and read the name back, `rounds` times, failing
        /// if the name ever belongs to a different protocol.
        fn hammer(number: i32, expect: &[u8], rounds: usize) {
            for _ in 0..rounds {
                // Takes a plain integer; returns a pointer into this
                // thread's own block.
                let p = crate::netdb::getprotobynumber(number);
                assert!(!p.is_null(), "no entry for protocol {number}");
                // SAFETY: non-null result, and `p_name` points at this
                // thread's NUL-terminated name buffer.
                let name = unsafe {
                    let n = (*p).p_name;
                    core::slice::from_raw_parts(n, crate::string::strlen(n))
                };
                assert_eq!(name, expect, "protocol {number} read another thread's name");
            }
        }

        const ROUNDS: usize = 2000;
        let other = std::thread::spawn(|| hammer(17, b"udp", ROUNDS));
        hammer(6, b"tcp", ROUNDS);
        other.join().expect("child thread panicked");
    }
}
