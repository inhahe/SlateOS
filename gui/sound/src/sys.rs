//! The C library's `ioctl`, for the sound devices ([`crate::pcm`]'s and
//! [`crate::mixer`]'s): one call, whose payload is checked against the size
//! its request encodes before the kernel is handed a pointer to it.
//!
//! The C library's and not the system call's, for the reason
//! [`crate::pcm`]'s device gives: which system-call table a process's
//! `syscall` instruction reaches is decided when its binary is loaded, and
//! only the C library knows which one its process has.

use std::ffi::{c_int, c_ulong, c_void};
use std::os::fd::RawFd;

use crate::pcm::Errno;

unsafe extern "C" {
    /// The C library's `ioctl`. Declared variadic, as glibc and musl declare
    /// it, so the call is right for theirs; lane D's takes the same three
    /// arguments in the same registers.
    #[link_name = "ioctl"]
    fn c_ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

/// The payload size `request` encodes: `_IOC`'s bits 16 to 29.
#[must_use]
pub const fn size_of_request(request: u32) -> usize {
    // Fourteen bits: no target this builds for has a `usize` narrower.
    ((request >> 16) & 0x3FFF) as usize
}

/// `ioctl(fd, request, payload)`, through the C library.
///
/// `payload` must be exactly the size `request` encodes -- empty for a
/// request that carries none -- and is refused with [`Errno::EINVAL`],
/// without a call, when it is not: the kernel reads and writes as many
/// bytes as the request says, so a shorter buffer would be written past.
///
/// # Errors
///
/// The kernel's errno, or [`Errno::EINVAL`] for a payload of the wrong
/// size.
pub fn ioctl(fd: RawFd, request: u32, payload: &mut [u8]) -> Result<(), Errno> {
    if payload.len() != size_of_request(request) {
        return Err(Errno::EINVAL);
    }
    let arg: *mut c_void = if payload.is_empty() {
        std::ptr::null_mut()
    } else {
        payload.as_mut_ptr().cast()
    };
    // SAFETY: `ioctl(fd, request, arg)` through the C library. `arg` is null
    // for a request that carries no payload, and otherwise points at
    // `payload`, which is live and exclusively borrowed for the call and --
    // checked above -- exactly the size `request` encodes, which is what the
    // kernel reads and writes for it; it keeps no pointer past the call. A
    // descriptor that is not open, or not a sound device, is answered with
    // an errno (`EBADF`, `ENOTTY`), not undefined behaviour.
    let ret = unsafe { c_ioctl(fd, c_ulong::from(request), arg) };
    if ret < 0 {
        Err(Errno(
            std::io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(Errno::EINVAL.0),
        ))
    } else {
        Ok(())
    }
}
