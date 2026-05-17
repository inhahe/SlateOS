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
    clippy::all,
    clippy::pedantic,
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
#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_Backtrace(
    _callback: extern "C" fn(*mut _Unwind_Context, *mut c_void) -> i32,
    _data: *mut c_void,
) -> i32 {
    URC_END_OF_STACK
}

/// Get the instruction pointer from an unwind context.
/// Stub: returns 0 (unknown address).
#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_GetIP(_context: *mut _Unwind_Context) -> usize {
    0
}

/// Find the start address of the function enclosing the given IP.
/// Stub: returns 0 (unknown — disables symbol_address in backtrace).
#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_FindEnclosingFunction(_pc: *mut c_void) -> *mut c_void {
    core::ptr::null_mut()
}

/// Get the canonical frame address from an unwind context.
/// Stub: returns 0 (unknown).
#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_GetCFA(_context: *mut _Unwind_Context) -> usize {
    0
}

// -----------------------------------------------------------------------
// Linux syscall() stub
//
// Rust std calls libc::syscall() directly for a few things:
//   - SYS_gettid (186) — get thread ID
//   - SYS_getrandom (318) — get random bytes
//   - SYS_futex (202) — futex operations
//
// Our POSIX layer provides dedicated functions for these (gettid,
// getrandom, futex), but std also calls the raw syscall() wrapper.
// This stub maps the most common Linux syscall numbers to our OS's
// equivalents via the dedicated POSIX functions.
// -----------------------------------------------------------------------

// Linux syscall numbers (x86_64)
const SYS_GETTID: i64 = 186;
const SYS_GETRANDOM: i64 = 318;
const SYS_FUTEX: i64 = 202;

// Our OS syscall numbers
const OUROS_SYS_TASK_ID: u64 = 2;

/// Issue a raw syscall with up to 3 arguments using our OS's ABI.
///
/// # Safety
///
/// Caller must provide valid arguments for the syscall number.
#[inline(always)]
unsafe fn raw_syscall3(nr: u64, a0: u64, a1: u64, a2: u64) -> i64 {
    let ret: i64;
    // SAFETY: SYSCALL instruction with our OS's ABI.
    // RAX = syscall number, RDI/RSI/RDX = args 0-2.
    // SYSCALL clobbers RCX (saves RIP) and R11 (saves RFLAGS).
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") a0,
            in("rsi") a1,
            in("rdx") a2,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Linux-compatible syscall() dispatcher.
///
/// Maps commonly-used Linux syscall numbers to our OS's equivalents.
/// Variadic arguments are captured via a fixed-size register read.
///
/// # Safety
///
/// Arguments must be valid for the requested syscall.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn syscall(num: i64, arg0: u64, arg1: u64, arg2: u64) -> i64 {
    match num {
        SYS_GETTID => unsafe { raw_syscall3(OUROS_SYS_TASK_ID, 0, 0, 0) },
        SYS_GETRANDOM => {
            // std calls getrandom(buf, len, flags) — delegate to our
            // POSIX getrandom() which is already linked.
            unsafe extern "C" {
                fn getrandom(buf: *mut u8, len: usize, flags: u32) -> isize;
            }
            unsafe { getrandom(arg0 as *mut u8, arg1 as usize, arg2 as u32) as i64 }
        }
        SYS_FUTEX => {
            // Delegate to our POSIX futex() implementation.
            unsafe extern "C" {
                fn futex(
                    uaddr: *mut u32,
                    op: i32,
                    val: u32,
                    timeout: u64,
                    uaddr2: *mut u32,
                    val3: u32,
                ) -> i32;
            }
            unsafe { futex(arg0 as *mut u32, arg1 as i32, arg2 as u32, 0, core::ptr::null_mut(), 0) as i64 }
        }
        _ => {
            // Unknown syscall — return ENOSYS (-38).
            -38
        }
    }
}

// -----------------------------------------------------------------------
// pthread stubs
//
// These are Linux/GNU-specific pthread functions that std uses for
// stack overflow detection.  Our OS doesn't yet implement them.
// Returning errors causes std to skip stack guard setup, which is
// acceptable for now.
// -----------------------------------------------------------------------

/// Opaque pthread attribute type (matches musl's size).
#[repr(C)]
pub struct pthread_attr_t {
    _data: [u8; 56],
}

/// Get thread attributes for the current thread (Linux-specific _np).
/// Stub: returns -1 (ENOSYS — not available).
#[unsafe(no_mangle)]
pub extern "C" fn pthread_getattr_np(
    _thread: u64,
    _attr: *mut pthread_attr_t,
) -> i32 {
    -1 // ENOSYS
}

/// Get the guard size from thread attributes.
/// Stub: writes 0 (no guard) and returns 0 (success).
#[unsafe(no_mangle)]
pub extern "C" fn pthread_attr_getguardsize(
    _attr: *const pthread_attr_t,
    guardsize: *mut usize,
) -> i32 {
    if !guardsize.is_null() {
        // SAFETY: caller guarantees guardsize is valid.
        unsafe { *guardsize = 0; }
    }
    0
}

/// Get the stack address and size from thread attributes.
/// Stub: writes zeroes and returns 0.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_attr_getstack(
    _attr: *const pthread_attr_t,
    stackaddr: *mut *mut c_void,
    stacksize: *mut usize,
) -> i32 {
    if !stackaddr.is_null() {
        // SAFETY: caller guarantees stackaddr is valid.
        unsafe { *stackaddr = core::ptr::null_mut(); }
    }
    if !stacksize.is_null() {
        // SAFETY: caller guarantees stacksize is valid.
        unsafe { *stacksize = 0; }
    }
    0
}

// -----------------------------------------------------------------------
// POSIX function stubs
//
// Functions that Nushell (via std or dependency crates) references but
// that our POSIX layer doesn't provide yet.  These should be moved to
// proper implementations in the posix crate as the OS matures.
// -----------------------------------------------------------------------

/// Opaque sigset_t — matches musl's representation.
#[repr(C)]
pub struct sigset_t {
    _bits: [u64; 16],
}

/// Set the default signal mask for a spawn attributes object.
/// Stub: returns 0 (success, no-op — signals not yet implemented).
#[unsafe(no_mangle)]
pub extern "C" fn posix_spawnattr_setsigdefault(
    _attr: *mut c_void,
    _sigdefault: *const sigset_t,
) -> i32 {
    0 // success, no-op
}

/// Send a signal to a process group.
/// Stub: returns -1 with errno=ENOSYS (signals not yet implemented).
#[unsafe(no_mangle)]
pub extern "C" fn killpg(_pgrp: i32, _sig: i32) -> i32 {
    // TODO: implement once POSIX signal support exists
    -1 // failure — ENOSYS
}

// -----------------------------------------------------------------------
// Panic handler — required for no_std staticlib
// -----------------------------------------------------------------------

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        // SAFETY: halt CPU — this stub library should never panic.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
    }
}
