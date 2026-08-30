//! `<sys/syscall.h>` / `<unistd.h>` — Linux syscall numbers and the
//! `syscall()` indirection.
//!
//! ## What the numbers in this module are
//!
//! The `SYS_*` constants defined *here* are the **Linux x86_64** syscall
//! numbers, verbatim (`read` = 0, `write` = 1, `getpid` = 39, …), taken
//! from Linux's `arch/x86/entry/syscalls/syscall_64.tbl`.  They are not
//! SlateOS numbers and do not correspond to anything our kernel would
//! accept.  They exist because portable C code writes
//! `syscall(SYS_gettid)` and expects the header to supply the constant.
//!
//! SlateOS's own numbers live in [`crate::syscall`] and are a completely
//! unrelated space — SlateOS `SYS_EXIT` is 1, which is Linux's `write`.
//! The two must never be confused, which is why this module deliberately
//! does **not** re-export [`crate::syscall`]'s constants: a glob
//! re-export here previously made `sys_syscall::SYS_EXIT` mean SlateOS's
//! 1 while `sys_syscall::SYS_WRITE` also meant 1, and forced an
//! awkwardly-named `SYS_EXIT_LINUX` to say what `SYS_EXIT` should have
//! said.  Code that wants a native number should say
//! `crate::syscall::SYS_EXIT` and mean it.
//!
//! ## What [`syscall`] does
//!
//! It is a **translation table, not a trap door.**  It maps a Linux
//! syscall number onto the corresponding function in this crate and
//! calls it.  It emphatically does not stuff the number into `RAX` and
//! execute `SYSCALL`: our kernel would decode Linux's 39 (`getpid`) as
//! whatever SlateOS numbers 39 (`SYS_GETRANDOM`'s neighbourhood), and the
//! resulting arbitrary syscall with mismatched arguments is exactly the
//! class of bug that is invisible until it corrupts something.
//!
//! Anything not in the table returns -1 with `ENOSYS`, which is what
//! Linux itself returns for an unimplemented number and what every
//! caller of `syscall()` is already required to handle — glibc's own
//! `syscall(2)` manual documents `ENOSYS` as the answer for a syscall
//! the running kernel lacks, and CPython, systemd and the like all probe
//! that way.

use crate::errno;

// ---------------------------------------------------------------------------
// Linux x86_64 syscall numbers
// ---------------------------------------------------------------------------
//
// Source: Linux 6.x `arch/x86/entry/syscalls/syscall_64.tbl`.

/// Linux `__NR_read`.
pub const SYS_READ: u64 = 0;

/// Linux `__NR_write`.
pub const SYS_WRITE: u64 = 1;

/// Linux `__NR_close`.
pub const SYS_CLOSE: u64 = 3;

/// Linux `__NR_brk`.
pub const SYS_BRK: u64 = 12;

/// Linux `__NR_rt_sigaction`.
pub const SYS_RT_SIGACTION: u64 = 13;

/// Linux `__NR_ioctl`.
pub const SYS_IOCTL: u64 = 16;

/// Linux `__NR_pipe`.
pub const SYS_PIPE: u64 = 22;

/// Linux `__NR_sched_yield`.
pub const SYS_SCHED_YIELD: u64 = 24;

/// Linux `__NR_getpid`.
pub const SYS_GETPID: u64 = 39;

/// Linux `__NR_fork`.
pub const SYS_FORK: u64 = 57;

/// Linux `__NR_execve`.
pub const SYS_EXECVE: u64 = 59;

/// Linux `__NR_exit`.
///
/// Note this is Linux's number.  SlateOS's own exit syscall is
/// [`crate::syscall::SYS_EXIT`] and is 1.
pub const SYS_EXIT: u64 = 60;

/// Linux `__NR_gettimeofday`.
pub const SYS_GETTIMEOFDAY: u64 = 96;

/// Linux `__NR_getuid`.
pub const SYS_GETUID: u64 = 102;

/// Linux `__NR_getgid`.
pub const SYS_GETGID: u64 = 104;

/// Linux `__NR_geteuid`.
pub const SYS_GETEUID: u64 = 107;

/// Linux `__NR_getegid`.
pub const SYS_GETEGID: u64 = 108;

/// Linux `__NR_getppid`.
pub const SYS_GETPPID: u64 = 110;

/// Linux `__NR_gettid`.
pub const SYS_GETTID: u64 = 186;

/// Linux `__NR_getrandom`.
pub const SYS_GETRANDOM: u64 = 318;

/// Linux `__NR_pidfd_send_signal`.
pub const SYS_PIDFD_SEND_SIGNAL: u64 = 424;

/// Linux `__NR_pidfd_open`.
pub const SYS_PIDFD_OPEN: u64 = 434;

// ---------------------------------------------------------------------------
// syscall()
// ---------------------------------------------------------------------------

/// The register-width signed type one `syscall()` argument or result
/// occupies.  C spells it `long`.
///
/// ## Why not [`core::ffi::c_long`]
///
/// Because `c_long` is **32 bits on the host this crate is tested on.**
/// `cargo test -p posix` builds for `x86_64-pc-windows-gnu`, which is
/// LLP64: `long` is `i32` there, while `long` on our own target
/// (`x86_64-slateos`, LP64, musl ABI) is `i64`.  Declaring the arguments
/// as `c_long` therefore produces a *different function* in the two
/// builds — one that truncates every pointer argument to 32 bits in the
/// build the tests actually exercise, so the tests would be passing on
/// code the target never runs.  That is the worst failure mode a test
/// can have.
///
/// `isize` is register width on both, is what `intptr_t` means, and is
/// the correct spelling of `long` on the LP64 target we ship.  The only
/// thing it is not is the correct spelling of `long` on a hypothetical
/// LLP64 SlateOS, which will not exist.
type SyscallArg = isize;

// Argument decoders.
//
// A `syscall()` argument arrives as a register-width word regardless of
// the type the target function declares, exactly as it would arriving in
// a register at a kernel entry point.  Narrowing it is therefore not a
// lossy conversion being papered over — it is the ABI, and the kernel
// does the identical truncation on the other side of a real `SYSCALL`.
// These helpers say that once, in one place, instead of scattering `as`
// casts (and the pedantic-lint suppressions they attract) through the
// dispatch table.

/// Reinterpret an argument register as a pointer.
#[inline]
fn ptr_arg(v: SyscallArg) -> *mut u8 {
    // `expose_provenance`/`with_exposed_provenance` is the sanctioned
    // spelling for an integer that genuinely *is* an address handed to
    // us from outside Rust's provenance model, which is precisely the
    // case for a syscall argument register.
    core::ptr::with_exposed_provenance_mut(trunc_usize(v))
}

/// Reinterpret an argument register as a machine word (a length, a size).
#[inline]
#[allow(clippy::cast_sign_loss)]
fn trunc_usize(v: SyscallArg) -> usize {
    v as usize
}

/// Narrow an argument register to a 32-bit signed value (an fd, a pid, a
/// signal number).
#[inline]
#[allow(clippy::cast_possible_truncation)]
fn trunc_i32(v: SyscallArg) -> i32 {
    v as i32
}

/// Narrow an argument register to a 32-bit flags word.
#[inline]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn trunc_u32(v: SyscallArg) -> u32 {
    v as u32
}

/// Widen a 32-bit signed result (the 0/-1 status most of the table
/// returns, or a pid) to the register width.
#[inline]
fn ret_i32(v: i32) -> SyscallArg {
    // `isize: From<i32>` does not exist — `isize` may be 16 bits in the
    // abstract — so this is a widening `as`, which cannot lose a bit on
    // any target with a 32-bit-or-wider pointer.
    #[allow(clippy::cast_possible_truncation)]
    {
        v as SyscallArg
    }
}

/// Widen a 32-bit unsigned result (a uid or gid) to the register width.
#[inline]
fn ret_u32(v: u32) -> SyscallArg {
    // Linux's getuid/getgid return the id zero-extended into the result
    // register; `-1` is not a valid id, so no value here is mistakable
    // for an error return.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    {
        v as SyscallArg
    }
}

/// `long syscall(long number, ...)` — invoke a syscall by Linux number.
///
/// Returns the syscall's result, or -1 with `errno` set to `ENOSYS` for
/// a number this layer does not translate.
///
/// ## Why fixed arguments rather than a variadic
///
/// C declares this as variadic (`long syscall(long number, ...)`), but
/// on x86-64 SysV a variadic call and a call to a function taking seven
/// fixed `long`s place their arguments in exactly the same registers —
/// `RDI, RSI, RDX, RCX, R8, R9` plus the stack — and neither the caller
/// nor the callee of a `syscall()` ever inspects `AL` (the vararg
/// FP-register count) because every argument is integral.  So a
/// seven-fixed-argument definition is ABI-compatible with every existing
/// C call site, and it avoids `#![feature(c_variadic)]`, which this crate
/// does not enable.  (The arguments are typed [`SyscallArg`] rather than
/// [`core::ffi::c_long`] — see that alias for why `c_long` would be a
/// trap.)
///
/// The cost is that arguments a caller did not pass are read as garbage
/// registers.  That is harmless here precisely because the dispatch is a
/// table: each arm consumes only the arguments its own syscall defines,
/// so an unpassed `a4` is never looked at.  A real trap-door
/// implementation could not make that guarantee.
///
/// ## The translation table
///
/// | Linux number | Mapped to | Notes |
/// |---|---|---|
/// | 24 `sched_yield` | [`crate::pthread::sched_yield`] | |
/// | 39 `getpid` | [`crate::process::getpid`] | |
/// | 96 `gettimeofday` | [`crate::time::gettimeofday`] | |
/// | 102/104/107/108 `get{u,g,eu,eg}id` | [`crate::unistd`] | |
/// | 110 `getppid` | [`crate::process::getppid`] | |
/// | 186 `gettid` | [`crate::process::gettid`] | CPython's `os.gettid` |
/// | 318 `getrandom` | [`crate::unistd::getrandom`] | CPython's `bootstrap_hash` |
/// | 424 `pidfd_send_signal` | [`crate::process::pidfd_send_signal`] | |
/// | 434 `pidfd_open` | [`crate::process::pidfd_open`] | CPython's `_Py_pidfd_open` |
///
/// These are the numbers real code reaches `syscall()` for: the ones
/// glibc either does not wrap at all (`gettid` before glibc 2.30,
/// `pidfd_open`, `pidfd_send_signal`) or wraps too recently to rely on
/// (`getrandom` before 2.25).  Everything else has a proper libc entry
/// point and portable code calls that instead.
///
/// `read`/`write`/`close`/`execve`/`fork`/`exit` are deliberately absent
/// even though their numbers are defined above.  A caller reaching them
/// through `syscall()` is doing so to bypass libc — typically inside a
/// `fork` child, or in a signal handler, where the libc wrapper's
/// bookkeeping is exactly what it is trying to avoid.  Silently routing
/// such a call back *into* the libc wrapper would defeat the caller's
/// purpose without telling it, so `ENOSYS` — which such code already
/// checks for, because it is written to cope with old kernels — is the
/// honest answer.  See `todo.txt`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn syscall(
    number: SyscallArg,
    a1: SyscallArg,
    a2: SyscallArg,
    a3: SyscallArg,
    a4: SyscallArg,
    _a5: SyscallArg,
    _a6: SyscallArg,
) -> SyscallArg {
    // Linux numbers are non-negative; a negative one cannot match.
    let Ok(n) = u64::try_from(number) else {
        errno::set_errno(errno::ENOSYS);
        return -1;
    };

    match n {
        SYS_SCHED_YIELD => ret_i32(crate::pthread::sched_yield()),
        SYS_GETPID => ret_i32(crate::process::getpid()),
        SYS_GETPPID => ret_i32(crate::process::getppid()),
        SYS_GETTID => ret_i32(crate::process::gettid()),
        SYS_GETUID => ret_u32(crate::unistd::getuid()),
        SYS_GETEUID => ret_u32(crate::unistd::geteuid()),
        SYS_GETGID => ret_u32(crate::unistd::getgid()),
        SYS_GETEGID => ret_u32(crate::unistd::getegid()),
        SYS_GETTIMEOFDAY => ret_i32(crate::time::gettimeofday(
            ptr_arg(a1).cast::<crate::time::Timeval>(),
            ptr_arg(a2).cast::<core::ffi::c_void>(),
        )),
        SYS_GETRANDOM => {
            // getrandom(2) already returns a signed count-or--1 that is
            // exactly register width, so it passes through unchanged — it
            // is the one entry here that is not a 0/-1 status.
            crate::unistd::getrandom(ptr_arg(a1), trunc_usize(a2), trunc_u32(a3))
        }
        SYS_PIDFD_OPEN => ret_i32(crate::process::pidfd_open(trunc_i32(a1), trunc_u32(a2))),
        SYS_PIDFD_SEND_SIGNAL => ret_i32(crate::process::pidfd_send_signal(
            trunc_i32(a1),
            trunc_i32(a2),
            ptr_arg(a3).cast_const().cast::<core::ffi::c_void>(),
            trunc_u32(a4),
        )),
        _ => {
            errno::set_errno(errno::ENOSYS);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: SyscallArg = 0;

    fn call1(n: u64, a1: SyscallArg) -> SyscallArg {
        syscall(
            SyscallArg::try_from(n).expect("test number fits"),
            a1,
            NONE,
            NONE,
            NONE,
            NONE,
            NONE,
        )
    }

    fn call0(n: u64) -> SyscallArg {
        call1(n, NONE)
    }

    #[test]
    fn test_linux_numbers_are_linux_numbers() {
        // Pinned against arch/x86/entry/syscalls/syscall_64.tbl.  These
        // must never drift towards SlateOS's own numbering — the whole
        // point of the module is that they are Linux's.
        assert_eq!(SYS_READ, 0);
        assert_eq!(SYS_WRITE, 1);
        assert_eq!(SYS_GETPID, 39);
        assert_eq!(SYS_EXIT, 60);
        assert_eq!(SYS_GETTID, 186);
        assert_eq!(SYS_PIDFD_OPEN, 434);
        // SlateOS's exit is 1, which is Linux's write.  If this ever
        // becomes equal, someone has re-merged the two number spaces.
        assert_ne!(SYS_EXIT, crate::syscall::SYS_EXIT);
        assert_eq!(crate::syscall::SYS_EXIT, SYS_WRITE);
    }

    #[test]
    fn test_linux_numbers_distinct() {
        let vals = [
            SYS_READ,
            SYS_WRITE,
            SYS_CLOSE,
            SYS_BRK,
            SYS_RT_SIGACTION,
            SYS_IOCTL,
            SYS_PIPE,
            SYS_SCHED_YIELD,
            SYS_GETPID,
            SYS_FORK,
            SYS_EXECVE,
            SYS_EXIT,
            SYS_GETTIMEOFDAY,
            SYS_GETUID,
            SYS_GETGID,
            SYS_GETEUID,
            SYS_GETEGID,
            SYS_GETPPID,
            SYS_GETTID,
            SYS_GETRANDOM,
            SYS_PIDFD_SEND_SIGNAL,
            SYS_PIDFD_OPEN,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j], "SYS_ constants must be distinct");
            }
        }
    }

    /// An unmapped number must be ENOSYS, not a wild dispatch.  1000 is
    /// past the end of the Linux table on every architecture.
    #[test]
    fn test_unknown_number_is_enosys() {
        errno::set_errno(0);
        assert_eq!(call0(1000), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    /// A negative number cannot be a Linux syscall and must not be
    /// reinterpreted as a huge unsigned one.
    #[test]
    fn test_negative_number_is_enosys() {
        errno::set_errno(0);
        assert_eq!(syscall(-1, NONE, NONE, NONE, NONE, NONE, NONE), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    /// The numbers we deliberately do *not* translate must report
    /// ENOSYS rather than quietly re-entering the libc wrapper.  If a
    /// future change adds them, this test is the reminder to document
    /// why.
    #[test]
    fn test_bypass_syscalls_are_not_silently_wrapped() {
        for n in [
            SYS_READ, SYS_WRITE, SYS_CLOSE, SYS_EXECVE, SYS_FORK, SYS_EXIT,
        ] {
            errno::set_errno(0);
            assert_eq!(call0(n), -1, "syscall({n}) must not dispatch");
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }
    }

    /// `syscall(SYS_getpid)` must agree with `getpid()` — the whole
    /// point of the indirection is that it is the same call.
    #[test]
    fn test_getpid_matches_wrapper() {
        assert_eq!(call0(SYS_GETPID), ret_i32(crate::process::getpid()));
    }

    /// Likewise `gettid`, which is the reason CPython links `syscall` at
    /// all (glibc gained a `gettid` wrapper only in 2.30).
    #[test]
    fn test_gettid_matches_wrapper() {
        assert_eq!(call0(SYS_GETTID), ret_i32(crate::process::gettid()));
    }

    /// `sched_yield` is always allowed to succeed, so the indirection
    /// must return its 0 rather than the ENOSYS default.
    #[test]
    fn test_sched_yield_dispatches() {
        errno::set_errno(0);
        assert_eq!(call0(SYS_SCHED_YIELD), 0);
    }

    /// `getrandom`'s return is a byte count, not a 0/-1 status, so it
    /// must pass through at full width.  Filling a small buffer is the
    /// cheapest way to prove the value is not being truncated to an
    /// `i32` status somewhere in the dispatch.
    #[test]
    fn test_getrandom_returns_a_count_not_a_status() {
        let mut buf = [0u8; 16];
        let addr =
            SyscallArg::try_from(buf.as_mut_ptr().expose_provenance()).expect("address fits");
        let n = syscall(
            SyscallArg::try_from(SYS_GETRANDOM).expect("fits"),
            addr,
            16,
            0,
            NONE,
            NONE,
            NONE,
        );
        let direct = crate::unistd::getrandom(buf.as_mut_ptr(), 16, 0);
        // Whatever getrandom does on this build (host or target), the
        // indirection must produce the identical value.
        assert_eq!(n, direct);
    }
}
