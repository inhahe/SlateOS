//! Per-thread storage for the libc functions that POSIX defines as
//! returning a pointer into a static buffer.
//!
//! ## Why this exists
//!
//! A family of standard functions — `strerror`, `gmtime`, `localtime`,
//! `asctime`, `ctime`, `inet_ntoa`, `gethostbyname`, `getservbyname`,
//! `getprotobyname`, … — return `*mut T` pointing at storage the *library*
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
    pub errno: i32,
}

impl PerThread {
    /// The initial state of a fresh thread's block.
    ///
    /// Must be bit-identical to all-zero: a thread's block is carved out of
    /// fresh anonymous memory and is never explicitly initialised.
    pub const ZERO: Self = Self { errno: 0 };
}

/// Bytes reserved for the per-thread block, rounded up so that placing it
/// at `TP + TCB_SIZE` (both multiples of 16) keeps the next thing aligned.
///
/// `crate::tls::TlsImage::reserve` adds this to every thread's mapping.
pub const BLOCK_SIZE: u64 = (size_of::<PerThread>() as u64).next_multiple_of(16);

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
    // The block is placed immediately above the TCB by
    // `TlsImage::reserve`/`thread_pointer`.
    (tp.wrapping_add(crate::tls::TCB_SIZE)) as *mut PerThread
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
    use super::{current, PerThread, BLOCK_SIZE};

    #[test]
    fn block_size_is_a_multiple_of_sixteen() {
        assert_eq!(BLOCK_SIZE % 16, 0);
        assert!(BLOCK_SIZE >= core::mem::size_of::<PerThread>() as u64);
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
}
