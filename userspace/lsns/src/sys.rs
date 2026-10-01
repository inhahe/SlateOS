//! The C library calls `lsns` makes that Rust's standard library does not
//! wrap, reached through the C ABI as the one-libc rule requires
//! (design-decisions §768): the nsfs `ioctl`s that relate a namespace to its
//! parent and owner, a netlink request for a network namespace's ID, and
//! `getpwuid`.
//!
//! On a host that is not Unix none of these exist: every `ioctl` fails with
//! `ENOTTY`, there is no netlink, and no user has a name. The differential
//! harness runs under WSL, where `cfg(unix)` holds.

use std::fs::File;

/// `NS_GET_USERNS`: the user namespace that owns this one.
pub const NS_GET_USERNS: u64 = 0xb701;
/// `NS_GET_PARENT`: the parent of a PID or user namespace.
pub const NS_GET_PARENT: u64 = 0xb702;
/// `NS_GET_NSTYPE`: the namespace's `CLONE_NEW*` type.
pub const NS_GET_NSTYPE: u64 = 0xb703;
/// `NS_GET_OWNER_UID`: the UID that owns a user namespace.
pub const NS_GET_OWNER_UID: u64 = 0xb704;

/// `ENOTTY`, which is also what a host without the ioctls reports.
pub const ENOTTY: i32 = 25;

#[cfg(unix)]
mod imp {
    use std::ffi::c_char;

    /// The first field of `struct passwd`, which both C libraries put first.
    #[repr(C)]
    pub struct Passwd {
        pub pw_name: *const c_char,
    }

    unsafe extern "C" {
        pub fn ioctl(fd: i32, request: u64, arg: *mut u8) -> i32;
        pub fn getpwuid(uid: u32) -> *const Passwd;
        pub fn socket(domain: i32, ty: i32, protocol: i32) -> i32;
        pub fn send(fd: i32, buf: *const u8, len: usize, flags: i32) -> isize;
        pub fn recv(fd: i32, buf: *mut u8, len: usize, flags: i32) -> isize;
    }
}

/// The raw descriptor of `file`.
#[cfg(unix)]
fn raw(file: &File) -> i32 {
    use std::os::fd::AsRawFd;
    file.as_raw_fd()
}

/// `ioctl(fd, request, arg)`: its non-negative answer, or `Err(errno)`.
///
/// # Safety
///
/// `arg` must be what `request` expects: null for a request that takes no
/// argument, or a pointer to live, writable memory of the size the request
/// writes.
///
/// # Errors
///
/// The `ioctl`'s `errno`.
unsafe fn ioctl_raw(file: &File, request: u64, arg: *mut u8) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        // SAFETY: `file` is open for the call, and the caller vouches for
        // `arg` as this function's contract requires.
        let rc = unsafe { imp::ioctl(raw(file), request, arg) };
        if rc < 0 {
            return Err(std::io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(ENOTTY));
        }
        Ok(rc)
    }
    #[cfg(not(unix))]
    {
        let _ = (file, request, arg);
        Err(ENOTTY)
    }
}

/// `ioctl(fd, request)` for a request that takes no argument and answers
/// with a number or a new descriptor.
///
/// # Errors
///
/// The `ioctl`'s `errno`.
pub fn ioctl_value(file: &File, request: u64) -> Result<i32, i32> {
    // SAFETY: the nsfs requests this is called with take no argument, so
    // the null pointer is never read or written.
    unsafe { ioctl_raw(file, request, std::ptr::null_mut()) }
}

/// `ioctl(fd, request)` for a request that answers with a new descriptor,
/// which is closed when the result is dropped.
///
/// # Errors
///
/// The `ioctl`'s `errno`.
pub fn ioctl_fd(file: &File, request: u64) -> Result<File, i32> {
    let fd = ioctl_value(file, request)?;
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        // SAFETY: the kernel has just returned `fd` as a new descriptor that
        // nothing else owns; the `File` takes that ownership and closes it.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(ENOTTY)
    }
}

/// `ioctl(fd, NS_GET_OWNER_UID, &uid)`.
///
/// # Errors
///
/// The `ioctl`'s `errno`.
pub fn ioctl_owner_uid(file: &File) -> Result<u32, i32> {
    let mut uid: u32 = 0;
    // SAFETY: `NS_GET_OWNER_UID` writes one `uid_t`, four bytes, and `uid`
    // is a live `u32` for the whole call.
    unsafe { ioctl_raw(file, NS_GET_OWNER_UID, (&raw mut uid).cast::<u8>()) }?;
    Ok(uid)
}

/// `getpwuid(uid)->pw_name`: the user's name, if the password database has
/// the user.
#[must_use]
pub fn getpwuid_name(uid: u32) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        // SAFETY: `getpwuid` returns null or a pointer to a static entry that
        // stays valid until the next such call; it is read at once.
        let pw = unsafe { imp::getpwuid(uid) };
        if pw.is_null() {
            return None;
        }
        // SAFETY: `pw` is non-null and points to the C library's entry.
        let name = unsafe { (*pw).pw_name };
        if name.is_null() {
            return None;
        }
        // SAFETY: `pw_name` is a NUL-terminated string owned by the entry.
        Some(
            unsafe { std::ffi::CStr::from_ptr(name) }
                .to_bytes()
                .to_vec(),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = uid;
        None
    }
}

/// A `NETLINK_ROUTE` socket, for `RTM_GETNSID`.
pub struct Netlink {
    #[cfg(unix)]
    fd: std::os::fd::OwnedFd,
}

/// `AF_NETLINK`, `SOCK_RAW`, `NETLINK_ROUTE`.
#[cfg(unix)]
const AF_NETLINK: i32 = 16;
#[cfg(unix)]
const SOCK_RAW: i32 = 3;
#[cfg(unix)]
const NETLINK_ROUTE: i32 = 0;
/// `RTM_NEWNSID`, `RTM_GETNSID`.
#[cfg(unix)]
const RTM_NEWNSID: u16 = 88;
#[cfg(unix)]
const RTM_GETNSID: u16 = 90;
/// `NETNSA_NSID`, `NETNSA_FD`.
#[cfg(unix)]
const NETNSA_NSID: u16 = 1;
#[cfg(unix)]
const NETNSA_FD: u16 = 3;
/// `NLM_F_REQUEST`.
#[cfg(unix)]
const NLM_F_REQUEST: u16 = 1;

impl Netlink {
    /// `socket(AF_NETLINK, SOCK_RAW, NETLINK_ROUTE)`, if the system has one.
    #[must_use]
    pub fn open() -> Option<Self> {
        #[cfg(unix)]
        {
            use std::os::fd::FromRawFd;
            // SAFETY: a plain socket call; the result is checked before use.
            let fd = unsafe { imp::socket(AF_NETLINK, SOCK_RAW, NETLINK_ROUTE) };
            if fd < 0 {
                return None;
            }
            // SAFETY: `fd` is the new socket, owned by nothing else.
            Some(Netlink {
                fd: unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) },
            })
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    /// `get_netnsid_via_netlink_send_request` and `..._recv_response`: the
    /// ID the network namespace behind `target` has in ours -- `None` when
    /// the request or the answer fails (upstream's "unusable").
    #[must_use]
    pub fn get_nsid(&self, target: &File) -> Option<i32> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // struct nlmsghdr (16) + struct rtgenmsg padded (4) + an rtattr
            // holding the target's descriptor (8): NLMSG_SPACE(1) +
            // RTA_SPACE(4).
            let mut req = [0u8; 28];
            req[0..4].copy_from_slice(&28u32.to_ne_bytes());
            req[4..6].copy_from_slice(&RTM_GETNSID.to_ne_bytes());
            req[6..8].copy_from_slice(&NLM_F_REQUEST.to_ne_bytes());
            // rtgen_family = AF_UNSPEC (0) at byte 16.
            req[20..22].copy_from_slice(&8u16.to_ne_bytes());
            req[22..24].copy_from_slice(&NETNSA_FD.to_ne_bytes());
            req[24..28].copy_from_slice(&target.as_raw_fd().to_ne_bytes());
            let fd = self.fd.as_raw_fd();
            // SAFETY: `req` is a live buffer of the length given.
            if unsafe { imp::send(fd, req.as_ptr(), req.len(), 0) } < 0 {
                return None;
            }
            // NLMSG_SPACE(1) + the larger of RTA_SPACE(4) and
            // RTA_SPACE(sizeof(struct nlmsgerr)), 40.
            let mut res = [0u8; 60];
            // SAFETY: `res` is a live, writable buffer of the length given.
            let n = unsafe { imp::recv(fd, res.as_mut_ptr(), res.len(), 0) };
            let len = usize::try_from(n).ok()?;
            let u16_at = |b: &[u8], at: usize| {
                let bytes = b.get(at..at.checked_add(2)?)?;
                <[u8; 2]>::try_from(bytes).ok().map(u16::from_ne_bytes)
            };
            let u32_at = |b: &[u8], at: usize| {
                let bytes = b.get(at..at.checked_add(4)?)?;
                <[u8; 4]>::try_from(bytes).ok().map(u32::from_ne_bytes)
            };
            // NLMSG_OK(nlh, len) && nlh->nlmsg_type == RTM_NEWNSID.
            let nlmsg_len = usize::try_from(u32_at(&res, 0)?).ok()?;
            if len < 16 || nlmsg_len < 16 || nlmsg_len > len || u16_at(&res, 4)? != RTM_NEWNSID {
                return None;
            }
            // NLMSG_PAYLOAD(nlh, sizeof(struct rtgenmsg)), and RTA_OK.
            let rtalen = nlmsg_len.checked_sub(20)?;
            let rta_len = usize::from(u16_at(&res, 20)?);
            if rtalen < 4 || rta_len < 4 || rta_len > rtalen || u16_at(&res, 22)? != NETNSA_NSID {
                return None;
            }
            u32_at(&res, 24).map(|v| v as i32)
        }
        #[cfg(not(unix))]
        {
            let _ = target;
            None
        }
    }
}
