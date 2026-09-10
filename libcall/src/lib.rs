//! The stateful parts of our C library, reached the one way that works.
//!
//! # Why this crate exists
//!
//! `posix` is compiled twice. Built as `libc.a` it contains real system calls,
//! and that is the copy every SlateOS program links. Listed as an ordinary
//! Rust dependency it compiles *again*, with every syscall replaced by a stub
//! answering `-ENOSYS`, because `toolchain/x86_64-slateos.json` sets
//! `"os": "linux"` while every syscall in `posix` is gated
//! `#[cfg(target_os = "none")]` for the bare-metal build. A program that does
//! both carries two libraries that disagree, and nothing warns it.
//!
//! `design-decisions.md` §768 drew the line: pure arithmetic over
//! caller-owned buffers may be a Rust dependency, anything with state must be
//! reached through the C ABI. `known-issues.md` →
//! `TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT` has
//! the measured chain: it is why `ssh` and `sshd` could not generate a host
//! key at all.
//!
//! # Why a crate rather than an `extern` block at each call site
//!
//! Because §768 already said what the lesson was, and it was not "remember to
//! use the C symbol":
//!
//! > The correct route and the incorrect one are indistinguishable at the call
//! > site, so the call site must not be where the choice is made.
//!
//! That was written after three crates independently got it wrong for entropy,
//! and the fix was one function — `randrange::fill_secret` — rather than three
//! corrected call sites. On 2026-09-10 three more crates got it wrong for
//! different calls: `userspace/swapon` (`swapon`, `swapoff`, and `get_errno`
//! twice), `userspace/powerctl` (`sync`) and `userspace/dhcpcd`
//! (`sethostname`, `get_errno`). All three were written by the lane that made
//! the §768 decision, in the six days after making it, and each one *looked*
//! right: `posix::unistd::swapon` is the most direct-looking route to the libc
//! and is the one route that does not reach it. This crate is the same remedy
//! as `fill_secret`, generalised — the choice is made once, here.
//!
//! # `errno` is returned, never fetched
//!
//! Every fallible call here returns `Result<(), i32>` carrying the `errno`
//! read *inside the same function*, immediately after the failure, through the
//! linked library's own `__errno_location`.
//!
//! This is the half of the bug that would have survived a narrower fix. The
//! call sites did `posix::errno::get_errno()`, a plain `pub fn` that reads the
//! **rlib's** `errno` cell — which `libc.a` never wrote. So a program called a
//! stubbed `swapon` that could not have worked, then read an `errno` from a
//! different copy of the library to find out why: both halves of the error
//! report wrong, independently. Returning the value makes fetching it from
//! anywhere unnecessary, so the wrong fetch has nowhere left to live.
//!
//! # The host arms are not stubs by accident
//!
//! On a non-`unix` host there is no Slate kernel, and every fallible call here
//! returns [`ENOSYS`] rather than pretending. That is the same choice
//! `randrange::fill_from_kernel`'s host arm makes, and for the same reason: a
//! host test that reaches for a kernel facility should see it decline, which
//! is what makes "fails when there is no kernel" a testable property instead
//! of an assumption.

#![no_std]

use core::ffi::CStr;

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------
//
// These are pure integers, and §768 permits naming `posix`'s copies directly
// — lane A confirmed on 2026-09-10 that a `pub const` cannot instantiate a
// second libc. They are restated here anyway, because a caller that reached
// for `posix::errno::EPERM` would need `posix` in its `[dependencies]`, and
// the whole point is that these crates no longer have that line at all. The
// values cannot drift: `constants_agree_with_posix` in the test module below
// asserts each one against `posix`'s, through a dev-dependency that is not
// linked into any program.

/// Operation not permitted.
pub const EPERM: i32 = 1;
/// No such file or directory.
pub const ENOENT: i32 = 2;
/// Bad address.
pub const EFAULT: i32 = 14;
/// Invalid argument.
pub const EINVAL: i32 = 22;
/// Function not implemented.
pub const ENOSYS: i32 = 38;

/// `swapon`: use the priority in the low bits rather than an automatic one.
pub const SWAP_FLAG_PREFER: i32 = 0x8000;
/// `swapon`: discard freed pages back to the device.
pub const SWAP_FLAG_DISCARD: i32 = 0x1_0000;
/// The bits of a `swapon` flag word that hold the priority.
pub const SWAP_FLAG_PRIO_MASK: i32 = 0x7FFF;

/// `klogctl`: read everything currently in the ring, without consuming it.
/// Needs no capability -- the ring is readable by anyone under Linux's default
/// `dmesg_restrict=0`.
pub const SYSLOG_ACTION_READ_ALL: i32 = 3;
/// `klogctl`: advance the clear floor past everything currently buffered.
/// Needs `CAP_SYSLOG`.
pub const SYSLOG_ACTION_CLEAR: i32 = 5;
/// `klogctl`: the ring's total capacity in bytes.
pub const SYSLOG_ACTION_SIZE_BUFFER: i32 = 10;

// ---------------------------------------------------------------------------
// The symbols themselves
// ---------------------------------------------------------------------------

/// The linked C library's symbols, declared once.
///
/// A separate module rather than an `extern` block inside each wrapper, so
/// that no wrapper shadows the symbol it is wrapping — `pub fn swapon` calling
/// an `extern "C" fn swapon` declared in its own body works, but reads as a
/// recursive call to anyone who has not spotted the inner item.
#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        pub fn sync();
        pub fn swapon(path: *const u8, swapflags: i32) -> i32;
        pub fn swapoff(path: *const u8) -> i32;
        pub fn sethostname(name: *const u8, len: usize) -> i32;
        pub fn setdomainname(name: *const u8, len: usize) -> i32;
        pub fn klogctl(cmd: i32, buf: *mut u8, len: i32) -> i32;
        pub fn __errno_location() -> *mut i32;
    }
}

/// This thread's `errno`, read from the library that just failed.
#[cfg(unix)]
fn last_errno() -> i32 {
    // SAFETY: `__errno_location` is the glibc/musl convention our `posix`
    // implements: it returns a valid, aligned, thread-local `*mut i32` that
    // stays valid for the life of the thread, and the read is of an `i32` the
    // library has already initialised.
    unsafe { *sys::__errno_location() }
}

// ---------------------------------------------------------------------------
// Wrappers
// ---------------------------------------------------------------------------

/// Flush every filesystem's dirty buffers to stable storage.
///
/// POSIX defines `sync(2)` as scheduling the writes, with no failure to
/// report, so there is nothing to return.
#[cfg(unix)]
pub fn sync() {
    // SAFETY: no arguments, no return value, and no memory reachable from
    // this process is read or written by the call.
    unsafe { sys::sync() };
}

/// Flush every filesystem's dirty buffers to stable storage.
///
/// The host has no Slate kernel and no filesystems of ours to flush. Doing
/// nothing is correct rather than a stub: `sync(2)` cannot report failure, so
/// there is no honest way for the host arm to say "I did not".
#[cfg(not(unix))]
pub fn sync() {}

/// Start swapping to `path`.
///
/// # Errors
///
/// The `errno` set by `swapon(2)`: `EPERM` without `CAP_SYS_ADMIN`, `ENOENT`
/// if the path does not exist, `EINVAL` for a malformed flag word or a file
/// that is not swap, `EFAULT` for a bad pointer, `ENOSYS` if this kernel has
/// no swap.
#[cfg(unix)]
pub fn swapon(path: &CStr, flags: i32) -> Result<(), i32> {
    // SAFETY: `CStr` guarantees a NUL terminator, and `path` outlives the
    // call, so the library reads a valid C string and nothing past it.
    let rc = unsafe { sys::swapon(path.as_ptr().cast::<u8>(), flags) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

/// Start swapping to `path`.
///
/// # Errors
///
/// Always [`ENOSYS`]: the host has no Slate kernel to ask.
#[cfg(not(unix))]
pub fn swapon(_path: &CStr, _flags: i32) -> Result<(), i32> {
    Err(ENOSYS)
}

/// Stop swapping to `path`.
///
/// # Errors
///
/// The `errno` set by `swapoff(2)`; see [`swapon`].
#[cfg(unix)]
pub fn swapoff(path: &CStr) -> Result<(), i32> {
    // SAFETY: as for `swapon` — `CStr` guarantees the terminator and `path`
    // outlives the call.
    let rc = unsafe { sys::swapoff(path.as_ptr().cast::<u8>()) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

/// Stop swapping to `path`.
///
/// # Errors
///
/// Always [`ENOSYS`]: the host has no Slate kernel to ask.
#[cfg(not(unix))]
pub fn swapoff(_path: &CStr) -> Result<(), i32> {
    Err(ENOSYS)
}

/// Set the system hostname to `name`.
///
/// Takes bytes rather than `&str` deliberately: a hostname is OS-boundary
/// data, and forcing UTF-8 on it here would be the crate's own bug rather
/// than the kernel's.
///
/// # Errors
///
/// The `errno` set by `sethostname(2)`: `EPERM` without the capability,
/// `EINVAL` if the name is longer than the kernel accepts, `EFAULT` for a bad
/// pointer, `ENOSYS` while libc has no syscall wired for it.
#[cfg(unix)]
pub fn sethostname(name: &[u8]) -> Result<(), i32> {
    // SAFETY: the pointer and length handed over are exactly `name`'s own, so
    // the library reads only within it. It is not required to be
    // NUL-terminated; the length is the bound.
    let rc = unsafe { sys::sethostname(name.as_ptr(), name.len()) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

/// Set the system hostname to `name`.
///
/// # Errors
///
/// Always [`ENOSYS`]: the host has no Slate kernel to name.
#[cfg(not(unix))]
pub fn sethostname(_name: &[u8]) -> Result<(), i32> {
    Err(ENOSYS)
}

/// Set the system NIS domain name to `name`.
///
/// # Errors
///
/// As [`sethostname`]; the kernel documents the same capability and the same
/// errors for both.
#[cfg(unix)]
pub fn setdomainname(name: &[u8]) -> Result<(), i32> {
    // SAFETY: as for `sethostname` — pointer and length are `name`'s own.
    let rc = unsafe { sys::setdomainname(name.as_ptr(), name.len()) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

/// Set the system NIS domain name to `name`.
///
/// # Errors
///
/// Always [`ENOSYS`]: the host has no Slate kernel to name.
#[cfg(not(unix))]
pub fn setdomainname(_name: &[u8]) -> Result<(), i32> {
    Err(ENOSYS)
}

/// How many bytes the kernel log ring can hold.
///
/// # Errors
///
/// The `errno` set by `klogctl(2)`.
#[cfg(unix)]
pub fn klog_size() -> Result<usize, i32> {
    // SAFETY: SIZE_BUFFER reads no user memory, so a null pointer and a zero
    // length are what the call expects.
    let rc = unsafe { sys::klogctl(SYSLOG_ACTION_SIZE_BUFFER, core::ptr::null_mut(), 0) };
    usize::try_from(rc).map_err(|_| last_errno())
}

/// How many bytes the kernel log ring can hold.
///
/// # Errors
///
/// Always [`ENOSYS`]: the host has no Slate kernel and no ring to size.
#[cfg(not(unix))]
pub fn klog_size() -> Result<usize, i32> {
    Err(ENOSYS)
}

/// Copy everything currently in the kernel log ring into `buf`.
///
/// Returns the number of bytes written. Does **not** consume the ring: this is
/// `SYSLOG_ACTION_READ_ALL`, the action `dmesg` uses, so running it twice gives
/// the same messages twice rather than nothing the second time.
///
/// # Errors
///
/// The `errno` set by `klogctl(2)`.
#[cfg(unix)]
pub fn klog_read_all(buf: &mut [u8]) -> Result<usize, i32> {
    let Ok(len) = i32::try_from(buf.len()) else {
        return Err(EINVAL);
    };
    // SAFETY: the pointer and length handed over are exactly `buf`'s own, so
    // the library writes only within it.
    let rc = unsafe { sys::klogctl(SYSLOG_ACTION_READ_ALL, buf.as_mut_ptr(), len) };
    usize::try_from(rc).map_err(|_| last_errno())
}

/// Copy everything currently in the kernel log ring into `buf`.
///
/// # Errors
///
/// Always [`ENOSYS`]: the host has no Slate kernel and no ring to read.
#[cfg(not(unix))]
pub fn klog_read_all(_buf: &mut [u8]) -> Result<usize, i32> {
    Err(ENOSYS)
}

/// Discard everything currently in the kernel log ring.
///
/// # Errors
///
/// The `errno` set by `klogctl(2)` -- in particular `EPERM` without
/// `CAP_SYSLOG`, which is the common case and worth reporting as itself rather
/// than guessed at.
#[cfg(unix)]
pub fn klog_clear() -> Result<(), i32> {
    // SAFETY: CLEAR reads no user memory; null and zero are what it expects.
    let rc = unsafe { sys::klogctl(SYSLOG_ACTION_CLEAR, core::ptr::null_mut(), 0) };
    if rc >= 0 { Ok(()) } else { Err(last_errno()) }
}

/// Discard everything currently in the kernel log ring.
///
/// # Errors
///
/// Always [`ENOSYS`]: the host has no Slate kernel and no ring to clear.
#[cfg(not(unix))]
pub fn klog_clear() -> Result<(), i32> {
    Err(ENOSYS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every constant here equals `posix`'s.
    ///
    /// Restating a value is how two sources of one truth start, and this lane
    /// has spent the week fixing exactly that shape. `posix` is a
    /// dev-dependency, so this comparison exists in the test binary and in no
    /// program — which is the only reason restating them is acceptable at all.
    #[test]
    fn constants_agree_with_posix() {
        assert_eq!(EPERM, posix::errno::EPERM);
        assert_eq!(ENOENT, posix::errno::ENOENT);
        assert_eq!(EFAULT, posix::errno::EFAULT);
        assert_eq!(EINVAL, posix::errno::EINVAL);
        assert_eq!(ENOSYS, posix::errno::ENOSYS);
        assert_eq!(SWAP_FLAG_PREFER, posix::unistd::SWAP_FLAG_PREFER);
        assert_eq!(SWAP_FLAG_DISCARD, posix::unistd::SWAP_FLAG_DISCARD);
        assert_eq!(SWAP_FLAG_PRIO_MASK, posix::unistd::SWAP_FLAG_PRIO_MASK);
        assert_eq!(
            SYSLOG_ACTION_READ_ALL,
            posix::unistd::SYSLOG_ACTION_READ_ALL
        );
        assert_eq!(SYSLOG_ACTION_CLEAR, posix::unistd::SYSLOG_ACTION_CLEAR);
        assert_eq!(
            SYSLOG_ACTION_SIZE_BUFFER,
            posix::unistd::SYSLOG_ACTION_SIZE_BUFFER
        );
    }

    /// The priority mask and the prefer bit do not overlap.
    ///
    /// `swapon` builds its flag word as `PREFER | (priority & PRIO_MASK)`; if
    /// the two ever overlapped, a priority would silently clear the bit that
    /// makes it mean anything.
    #[test]
    fn the_prefer_bit_is_outside_the_priority_field() {
        assert_eq!(SWAP_FLAG_PREFER & SWAP_FLAG_PRIO_MASK, 0);
        assert_eq!(SWAP_FLAG_DISCARD & SWAP_FLAG_PRIO_MASK, 0);
        assert_eq!(SWAP_FLAG_DISCARD & SWAP_FLAG_PREFER, 0);
    }

    /// On a host, every fallible call declines rather than pretending.
    ///
    /// Asserted as a property of the whole surface rather than one call, so a
    /// wrapper added later without a host arm — or with one that returns
    /// `Ok(())` — fails here rather than in a boot test.
    #[cfg(not(unix))]
    #[test]
    fn the_host_arms_all_decline() {
        let path = c"/dev/null";
        assert_eq!(swapon(path, 0), Err(ENOSYS));
        assert_eq!(swapoff(path), Err(ENOSYS));
        assert_eq!(sethostname(b"host"), Err(ENOSYS));
        assert_eq!(setdomainname(b"domain"), Err(ENOSYS));
        assert_eq!(klog_size(), Err(ENOSYS));
        assert_eq!(klog_read_all(&mut [0u8; 8]), Err(ENOSYS));
        assert_eq!(klog_clear(), Err(ENOSYS));
        // `sync` has no failure to report on either arm; calling it here
        // asserts only that the host arm exists and does not panic.
        sync();
    }
}
