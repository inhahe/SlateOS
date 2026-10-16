//! A route netlink socket that hears the kernel announce network address
//! changes -- what agetty listens to when its issue shows an address
//! (`\4`, `\6`), to show it again when it changes.
//!
//! Linux's: SlateOS's C library has no `AF_NETLINK`, so there the socket
//! is refused and nothing is heard, which is what a caller must expect of
//! any kernel without it.

/// `RTMGRP_IPV4_IFADDR`: IPv4 addresses come and go.
pub const RTMGRP_IPV4_IFADDR: u32 = 0x10;
/// `RTMGRP_IPV6_IFADDR`: IPv6 addresses come and go.
pub const RTMGRP_IPV6_IFADDR: u32 = 0x100;

/// `NLMSG_ERROR`.
const NLMSG_ERROR: u16 = 2;
/// `NLMSG_DONE`.
const NLMSG_DONE: u16 = 3;
/// `sizeof (struct nlmsghdr)`.
const NLMSG_HDRLEN: usize = 16;

#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        pub fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
        pub fn bind(fd: i32, addr: *const SockaddrNl, len: u32) -> i32;
        pub fn recv(fd: i32, buf: *mut core::ffi::c_void, len: usize, flags: i32) -> isize;
        pub fn close(fd: i32) -> i32;
    }

    /// `struct sockaddr_nl`.
    #[repr(C)]
    pub struct SockaddrNl {
        pub nl_family: u16,
        pub nl_pad: u16,
        pub nl_pid: u32,
        pub nl_groups: u32,
    }

    /// `AF_NETLINK`.
    pub const AF_NETLINK: i32 = 16;
    /// `SOCK_RAW | SOCK_CLOEXEC`.
    pub const SOCK_RAW_CLOEXEC: i32 = 3 | 0o2_000_000;
    /// `NETLINK_ROUTE`.
    pub const NETLINK_ROUTE: i32 = 0;
    /// `MSG_DONTWAIT`.
    pub const MSG_DONTWAIT: i32 = 0x40;
}

/// A route netlink socket bound to hear `groups` (`RTMGRP_*`): the
/// descriptor, close-on-exec, the caller's to close.
///
/// Its address is the kernel's choice. C's agetty binds the process id,
/// which fails (`EADDRINUSE`) when the C library already holds a route
/// socket under it -- as `getaddrinfo` and `getifaddrs` do, behind the issue's
/// own `\4` and `\6` -- and then hears nothing.
///
/// # Errors
///
/// `EAFNOSUPPORT` where there is no netlink, `EPERM`, `EMFILE`;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn open_route(groups: u32) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        // SAFETY: three numbers in, no memory of ours.
        let fd = unsafe { sys::socket(sys::AF_NETLINK, sys::SOCK_RAW_CLOEXEC, sys::NETLINK_ROUTE) };
        if fd < 0 {
            return Err(crate::last_errno());
        }
        let addr = sys::SockaddrNl {
            nl_family: 16,
            nl_pad: 0,
            // "Any free address".
            nl_pid: 0,
            nl_groups: groups,
        };
        // SAFETY: `addr` is a whole `sockaddr_nl` alive for the call, which
        // only reads it, and the length is its size.
        let rc = unsafe { sys::bind(fd, &raw const addr, 12) };
        if rc < 0 {
            let errno = crate::last_errno();
            // SAFETY: `fd` is the socket made above, closed once.
            unsafe { sys::close(fd) };
            return Err(errno);
        }
        Ok(fd)
    }
    #[cfg(not(unix))]
    {
        let _ = groups;
        Err(crate::ENOSYS)
    }
}

/// One message from `fd` into `buf`, without waiting: its length, 0 when
/// there is none yet.
///
/// # Errors
///
/// The `errno` of `recv`, `EAGAIN` excepted; [`ENOSYS`](crate::ENOSYS) off
/// Unix.
pub fn receive(fd: i32, buf: &mut [u8]) -> Result<usize, i32> {
    #[cfg(unix)]
    {
        // SAFETY: `buf` is live and writable for its length, all the call
        // writes.
        let n = unsafe { sys::recv(fd, buf.as_mut_ptr().cast(), buf.len(), sys::MSG_DONTWAIT) };
        if n < 0 {
            let errno = crate::last_errno();
            return if errno == crate::EAGAIN {
                Ok(0)
            } else {
                Err(errno)
            };
        }
        Ok(usize::try_from(n).unwrap_or(0))
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, buf);
        Err(crate::ENOSYS)
    }
}

/// What the first message of a datagram says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said {
    /// No whole message.
    Nothing,
    /// The end of a dump, or an error: the socket has nothing more to say.
    End,
    /// Something changed.
    Change,
}

/// The first message in `buf`, judged as `NLMSG_OK` judges one: a header
/// whose length is a header at least and within what was received.
#[must_use]
pub fn first_message(buf: &[u8]) -> Said {
    let len = buf.get(..4).and_then(|b| <[u8; 4]>::try_from(b).ok());
    let kind = buf.get(4..6).and_then(|b| <[u8; 2]>::try_from(b).ok());
    let (Some(len), Some(kind)) = (len, kind) else {
        return Said::Nothing;
    };
    let len = usize::try_from(u32::from_ne_bytes(len)).unwrap_or(usize::MAX);
    if buf.len() < NLMSG_HDRLEN || len < NLMSG_HDRLEN || len > buf.len() {
        return Said::Nothing;
    }
    match u16::from_ne_bytes(kind) {
        NLMSG_DONE | NLMSG_ERROR => Said::End,
        _ => Said::Change,
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn message(len: u32, kind: u16) -> [u8; 16] {
        let mut m = [0u8; 16];
        m[..4].copy_from_slice(&len.to_ne_bytes());
        m[4..6].copy_from_slice(&kind.to_ne_bytes());
        m
    }

    #[test]
    fn a_message_is_a_change_unless_it_ends_the_talk() {
        // RTM_NEWADDR.
        assert_eq!(first_message(&message(16, 20)), Said::Change);
        assert_eq!(first_message(&message(16, NLMSG_DONE)), Said::End);
        assert_eq!(first_message(&message(16, NLMSG_ERROR)), Said::End);
    }

    #[test]
    fn a_message_that_is_not_whole_is_none() {
        assert_eq!(first_message(&[]), Said::Nothing);
        assert_eq!(first_message(&message(15, 20)), Said::Nothing);
        assert_eq!(first_message(&message(17, 20)), Said::Nothing);
        assert_eq!(first_message(&message(16, 20)[..12]), Said::Nothing);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_gives_a_socket_that_has_nothing_to_say_yet() {
        let fd = open_route(RTMGRP_IPV4_IFADDR);
        assert!(fd.is_ok(), "{fd:?}");
        let Ok(fd) = fd else { return };
        let mut buf = [0u8; 64];
        assert_eq!(receive(fd, &mut buf), Ok(0));
        // SAFETY: the socket made above, closed once.
        unsafe { sys::close(fd) };
    }
}
