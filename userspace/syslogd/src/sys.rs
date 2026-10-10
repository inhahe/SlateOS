//! The socket calls `syslogd` makes that std does not wrap: `SO_PASSCRED` on
//! the `/dev/log` socket and the `recvmsg` that reads each datagram with its
//! sender's credentials (`SCM_CREDENTIALS`) -- reached through the C ABI, the
//! linked library's, as the one-libc rule requires (design-decisions §768).
//! Binding and the rest are std's `UnixDatagram`, which calls the same
//! library.
//!
//! Linux-shaped targets only, SlateOS's included: there is no `/dev/log`
//! anywhere else, and the daemon says so rather than pretending.

use std::io;
use std::os::fd::RawFd;

use crate::record::Creds;

/// The largest datagram read whole. journald sizes its buffer to each
/// datagram; this reads into one buffer above Linux's default ceiling for a
/// Unix datagram (`net.core.wmem_default`, 208 KiB), and reports the rare
/// one that was longer rather than filing it cut without a word.
pub const MAX_DATAGRAM: usize = 256 * 1024;

const SOL_SOCKET: i32 = 1;
const SO_PASSCRED: i32 = 16;
const SCM_CREDENTIALS: i32 = 2;
/// `MSG_TRUNC` in the flags `recvmsg` hands back: the datagram was longer
/// than the buffer.
const MSG_TRUNC: i32 = 0x20;

#[repr(C)]
struct Iovec {
    base: *mut u8,
    len: usize,
}

/// `struct msghdr`, x86_64 Linux layout.
#[repr(C)]
struct Msghdr {
    name: *mut u8,
    namelen: u32,
    iov: *mut Iovec,
    iovlen: usize,
    control: *mut u8,
    controllen: usize,
    flags: i32,
}

/// `sizeof (struct cmsghdr)`: `cmsg_len` (a `size_t`), `cmsg_level`,
/// `cmsg_type`.
const CMSG_HEADER: usize = 16;

/// `sizeof (struct ucred)`: `pid`, `uid`, `gid`, four bytes each.
const UCRED: usize = 12;

mod ffi {
    use super::Msghdr;

    unsafe extern "C" {
        pub fn setsockopt(fd: i32, level: i32, name: i32, value: *const u8, len: u32) -> i32;
        pub fn recvmsg(fd: i32, msg: *mut Msghdr, flags: i32) -> isize;
    }
}

/// Ask the kernel to attach each sender's credentials to what `fd` receives.
///
/// # Errors
///
/// What `setsockopt` reports.
pub fn pass_credentials(fd: RawFd) -> io::Result<()> {
    let on: i32 = 1;
    // SAFETY: `on` is a live `i32` and the length given is its size; the
    // kernel copies it before the call returns.
    let rc =
        unsafe { ffi::setsockopt(fd, SOL_SOCKET, SO_PASSCRED, (&raw const on).cast::<u8>(), 4) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// One datagram into `buf`: its length, whether it was cut short (longer
/// than `buf`), and its sender's credentials when the kernel attached them.
///
/// # Errors
///
/// What `recvmsg` reports; `EINTR` is the caller's to retry.
pub fn receive(fd: RawFd, buf: &mut [u8]) -> io::Result<(usize, bool, Option<Creds>)> {
    let mut iov = Iovec {
        base: buf.as_mut_ptr(),
        len: buf.len(),
    };
    // Room for one `SCM_CREDENTIALS` message: a header and a `ucred`,
    // aligned, as `CMSG_SPACE (sizeof (struct ucred))` is -- 32 bytes.
    let mut control = [0u64; 4];
    let mut msg = Msghdr {
        name: std::ptr::null_mut(),
        namelen: 0,
        iov: &raw mut iov,
        iovlen: 1,
        control: control.as_mut_ptr().cast::<u8>(),
        controllen: std::mem::size_of_val(&control),
        flags: 0,
    };
    // SAFETY: `msg` points at `iov`, which points at `buf`, and at
    // `control`; all three outlive the call, and the lengths given are
    // theirs. The kernel writes at most those many bytes into each.
    let n = unsafe { ffi::recvmsg(fd, &raw mut msg, 0) };
    let Ok(len) = usize::try_from(n) else {
        return Err(io::Error::last_os_error());
    };
    let truncated = msg.flags & MSG_TRUNC != 0;
    Ok((len, truncated, credentials(&control, msg.controllen)))
}

/// The `SCM_CREDENTIALS` message in a control buffer `len` bytes long, if
/// it has one. Read field by field from the bytes rather than through a
/// cast, so alignment is never assumed.
fn credentials(control: &[u64; 4], len: usize) -> Option<Creds> {
    let bytes: Vec<u8> = control.iter().flat_map(|w| w.to_ne_bytes()).collect();
    let bytes = bytes.get(..len.min(bytes.len()))?;
    let cmsg_len = usize::from_ne_bytes(bytes.get(..8)?.try_into().ok()?);
    let level = i32::from_ne_bytes(bytes.get(8..12)?.try_into().ok()?);
    let kind = i32::from_ne_bytes(bytes.get(12..16)?.try_into().ok()?);
    if level != SOL_SOCKET || kind != SCM_CREDENTIALS || cmsg_len < CMSG_HEADER + UCRED {
        return None;
    }
    let pid = i32::from_ne_bytes(bytes.get(16..20)?.try_into().ok()?);
    let uid = u32::from_ne_bytes(bytes.get(20..24)?.try_into().ok()?);
    let gid = u32::from_ne_bytes(bytes.get(24..28)?.try_into().ok()?);
    Some(Creds { pid, uid, gid })
}
