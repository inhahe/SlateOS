//! Stub symbols needed by Rust std that our POSIX layer doesn't provide.
//!
//! These are symbols that the Rust standard library (built for a
//! linux-musl target) expects from system libraries but that our OS
//! doesn't yet implement.  Stubs allow linking to succeed; the
//! underlying features (backtrace, stack overflow detection) are
//! degraded but not critical.
//!
//! As our POSIX layer grows, symbols should be moved from here to
//! proper implementations in the posix crate.

#![no_std]
#![allow(
    clippy::missing_safety_doc,
    clippy::not_unsafe_ptr_arg_deref,
    non_camel_case_types,
    // `_Unwind_Backtrace` and friends are C ABI names fixed by the
    // unwinder interface, not ours to rename. `#[no_mangle]` used to
    // exempt them from this lint implicitly; now that the attribute is
    // `cfg_attr(target_os = "none", ...)` the host build has no
    // `no_mangle` and the exemption has to be stated.
    non_snake_case,
    clippy::all,
    clippy::pedantic
)]

use core::ffi::c_void;

// -----------------------------------------------------------------------
// Unwind / backtrace stubs
//
// Rust std uses libunwind for panic backtraces.  With panic=abort we
// never actually unwind, but std's backtrace-on-panic code still
// references these symbols.  Return "no frames" / "not available".
// -----------------------------------------------------------------------

/// Opaque unwind context — never dereferenced by our stubs.
pub struct _Unwind_Context {
    _private: [u8; 0],
}

/// _URC_END_OF_STACK — tells the caller there are no more frames.
const URC_END_OF_STACK: i32 = 5;

/// Walk the call stack, invoking `callback` for each frame.
/// Stub: immediately returns "end of stack" (no frames available).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _Unwind_Backtrace(
    _callback: extern "C" fn(*mut _Unwind_Context, *mut c_void) -> i32,
    _data: *mut c_void,
) -> i32 {
    URC_END_OF_STACK
}

/// Get the instruction pointer from an unwind context.
/// Stub: returns 0 (unknown address).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _Unwind_GetIP(_context: *mut _Unwind_Context) -> usize {
    0
}

/// Find the start address of the function enclosing the given IP.
/// Stub: returns 0 (unknown — disables symbol_address in backtrace).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _Unwind_FindEnclosingFunction(_pc: *mut c_void) -> *mut c_void {
    core::ptr::null_mut()
}

/// Get the canonical frame address from an unwind context.
/// Stub: returns 0 (unknown).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _Unwind_GetCFA(_context: *mut _Unwind_Context) -> usize {
    0
}

// -----------------------------------------------------------------------
// REMOVED 2026-09-13: syscall(), killpg(), posix_spawnattr_setsigdefault()
//
// All three were defined here AND in the posix crate, as `no_mangle` C
// symbols in two archives that are both linked into every userspace binary.
// `.cargo/config.toml` passes `--allow-multiple-definition`, so the duplicate
// was not an error -- the linker silently took whichever it saw first, and
// `-lstubs` is passed before rustc's own `-lc`.
//
// The three stubs were the WORSE half of each pair, and this file's own
// header says what should have happened: "As our POSIX layer grows, symbols
// should be moved from here to proper implementations in the posix crate."
// The moving happened; the deleting did not.
//
//   killpg                          stub: `-1`, and despite its doc comment
//                                   saying "errno=ENOSYS" it set no errno at
//                                   all, so a caller's perror() printed a
//                                   stale unrelated error.
//                                   posix: EINVAL on a negative group, a
//                                   checked negation, delegation to kill().
//
//   posix_spawnattr_setsigdefault   stub: `0` -- success, no-op. A caller
//                                   believed the spawned child would have
//                                   those signals reset to default.
//                                   posix: real, and already tested.
//
//   syscall                         stub took FOUR parameters where posix's
//                                   takes seven, so three argument registers
//                                   were silently dropped. It routed only
//                                   gettid/getrandom/futex, and its futex
//                                   arm hardcoded timeout=NULL, uaddr2=NULL
//                                   and val3=0 -- a FUTEX_WAIT with a
//                                   timeout waited forever.
//                                   posix's now routes 202 with all six
//                                   arguments; that arm was added in the same
//                                   commit that deleted this one, because
//                                   deleting it first would have removed
//                                   futex from `syscall()` entirely.
//
// What is left below is the four `_Unwind_*` stubs, which are still genuine:
// there is no unwinder, and with panic=abort nothing unwinds.
// -----------------------------------------------------------------------

// -----------------------------------------------------------------------
// Panic handler — required for no_std staticlib
// -----------------------------------------------------------------------

// Only define a panic handler in non-test builds; the test harness provides its own.
#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        // SAFETY: halt CPU — this stub library should never panic.
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

// -----------------------------------------------------------------------
// Tests
//
// This crate had none, and the untested-crates gate could not see that
// because its roots came from CLAUDE.md's lane table, which does not mention
// `toolchain`. What is left here is four stubs returning constants, and three
// of those constants are load-bearing rather than arbitrary: they are the
// values std's backtrace code reads to decide there is nothing to print.
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicBool, Ordering};

    static CALLED: AtomicBool = AtomicBool::new(false);

    extern "C" fn record(_ctx: *mut _Unwind_Context, _data: *mut c_void) -> i32 {
        CALLED.store(true, Ordering::SeqCst);
        0
    }

    #[test]
    fn backtrace_reports_end_of_stack_not_success() {
        // 5 is `_URC_END_OF_STACK`; 0 is `_URC_NO_REASON`. The difference is
        // whether std's walker learns the walk is OVER. Returning 0 would say
        // "nothing went wrong" without saying "there is nothing more", which
        // is not the same answer and is the plausible-looking simplification.
        assert_eq!(_Unwind_Backtrace(record, core::ptr::null_mut()), 5);
        assert_eq!(URC_END_OF_STACK, 5);
    }

    #[test]
    fn backtrace_never_invokes_the_callback() {
        // The contract that keeps the other three stubs safe. They are
        // documented as never dereferencing their context, and they are only
        // ever reached through a callback -- so a version that DID invoke it
        // would hand std a null context and get an address of 0 back for
        // every frame, which prints a backtrace of zeroes rather than none.
        CALLED.store(false, Ordering::SeqCst);
        let _ = _Unwind_Backtrace(record, core::ptr::null_mut());
        assert!(
            !CALLED.load(Ordering::SeqCst),
            "the stub invoked the callback; there are no frames to report"
        );
    }

    #[test]
    fn the_address_queries_answer_unknown_rather_than_a_plausible_address() {
        // 0 and NULL are what std reads as "no symbol". Any other value would
        // be a fabricated address that a symboliser would try to resolve.
        assert_eq!(_Unwind_GetIP(core::ptr::null_mut()), 0);
        assert_eq!(_Unwind_GetCFA(core::ptr::null_mut()), 0);
        assert!(_Unwind_FindEnclosingFunction(core::ptr::null_mut()).is_null());
    }
}
