//! The libc calls util-linux's `logger` makes that std does not wrap, reached
//! through the C ABI as the one-libc rule requires (design-decisions §768):
//! `getlogin`, `getpwuid`, `getuid`/`geteuid`, `gethostname`, `ntp_gettime`
//! (as `adjtimex`), name resolution for `-n`, and the `sendmsg` that attaches
//! a claimed PID to a local message (`SCM_CREDENTIALS`).
//!
//! Each has a host fallback for the Windows build the unit tests run on, where
//! none of these exist; the fallbacks answer "unknown", which the callers
//! already handle.

use std::net::SocketAddr;

/// `xgetlogin()` (util-linux `lib/pwdutils.c`): `getlogin()`, else the name
/// of the real uid, else `None`.
#[must_use]
pub fn xgetlogin() -> Option<Vec<u8>> {
    imp::getlogin().or_else(|| imp::getpwuid_name(imp::getuid()?))
}

/// `xgethostname()`: the host's name, or `None` if it cannot be read.
#[must_use]
pub fn hostname() -> Option<Vec<u8>> {
    imp::gethostname()
}

/// `ntp_gettime`'s verdict on the clock: `Some(maxerror)` in microseconds
/// when the kernel says `TIME_OK`, `None` otherwise -- including where the
/// call is unavailable, which upstream's `#ifdef HAVE_NTP_GETTIME` treats the
/// same way.
#[must_use]
pub fn synced_maxerror() -> Option<i64> {
    imp::adjtimex_maxerror()
}

/// Whether root may attach `pid` to a local message as its sender's PID:
/// upstream's `ctl->pid != getpid() && geteuid() == 0 && kill(ctl->pid, 0) ==
/// 0`, evaluated in that order, so `kill` is only reached as root. `pid` is
/// used exactly as given -- `--id=4294967295` wraps to -1, and `kill(-1, 0)`
/// asks about every process the caller may signal, as it does upstream. The
/// kernel has the last word: it refuses the credentials unless the sender may
/// claim them.
#[must_use]
pub fn may_claim(pid: i32) -> bool {
    i32::try_from(std::process::id()).ok() != Some(pid)
        && imp::geteuid() == Some(0)
        && imp::process_exists(pid)
}

/// `sendmsg(fd, {wire}, MSG_NOSIGNAL)` with one `SCM_CREDENTIALS` control
/// message naming `pid` -- upstream zeroes the buffer and sets only the PID,
/// so the claimed uid and gid are 0 -- on a connected Unix-domain socket.
///
/// # Errors
///
/// Whatever `sendmsg` reports; `EPERM` when the kernel will not let this
/// process claim `pid`.
#[cfg(unix)]
pub fn send_as(fd: std::os::fd::RawFd, wire: &[u8], pid: i32) -> std::io::Result<usize> {
    imp::send_as(fd, wire, pid)
}

/// `getaddrinfo(node, service, {AF_UNSPEC, socktype})`'s first address --
/// the only one `inet_socket` uses -- or `gai_strerror`'s text.
///
/// # Errors
///
/// The name or the service does not resolve.
pub fn resolve(node: &[u8], service: &[u8], socktype: i32) -> Result<SocketAddr, String> {
    imp::resolve(node, service, socktype)
}

#[cfg(unix)]
mod imp {
    use std::ffi::{CStr, CString, c_char};
    use std::io;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

    /// `struct passwd` as glibc lays it out on x86_64, and as the SlateOS libc
    /// declares it; only `pw_name` is read.
    #[repr(C)]
    struct Passwd {
        pw_name: *mut c_char,
        pw_passwd: *mut c_char,
        pw_uid: u32,
        pw_gid: u32,
        pw_gecos: *mut c_char,
        pw_dir: *mut c_char,
        pw_shell: *mut c_char,
    }

    /// `struct timex`, x86_64 Linux layout (208 bytes). Only `modes` is
    /// written and only `maxerror` read; the rest is room for the kernel.
    #[repr(C)]
    struct Timex {
        modes: u32,
        offset: i64,
        freq: i64,
        maxerror: i64,
        esterror: i64,
        status: i32,
        constant: i64,
        precision: i64,
        tolerance: i64,
        time: [i64; 2],
        tick: i64,
        ppsfreq: i64,
        jitter: i64,
        shift: i32,
        stabil: i64,
        jitcnt: i64,
        calcnt: i64,
        errcnt: i64,
        stbcnt: i64,
        tai: i32,
        reserved: [i32; 11],
    }
    const _: () = assert!(std::mem::size_of::<Timex>() == 208);

    /// `struct addrinfo`, as glibc lays it out and as `posix::socket`
    /// declares it (`scripts/check-libc-abi.py` holds the two together).
    #[repr(C)]
    struct AddrInfo {
        ai_flags: i32,
        ai_family: i32,
        ai_socktype: i32,
        ai_protocol: i32,
        ai_addrlen: u32,
        ai_addr: *mut u8,
        ai_canonname: *mut u8,
        ai_next: *mut AddrInfo,
    }

    /// `struct iovec`.
    #[repr(C)]
    struct Iovec {
        iov_base: *const u8,
        iov_len: usize,
    }

    /// `struct msghdr`, x86_64 layout (56 bytes, `msg_iovlen` and
    /// `msg_controllen` a `size_t`), as glibc declares it and as
    /// `posix::socket::Msghdr` does.
    #[repr(C)]
    struct Msghdr {
        msg_name: *mut u8,
        msg_namelen: u32,
        msg_iov: *const Iovec,
        msg_iovlen: usize,
        msg_control: *mut u8,
        msg_controllen: usize,
        msg_flags: i32,
    }
    const _: () = assert!(std::mem::size_of::<Msghdr>() == 56);

    /// Upstream's control buffer: a `struct cmsghdr` (16 bytes) and the
    /// `struct ucred` after it (`pid`, `uid`, `gid`), padded to
    /// `CMSG_SPACE(sizeof(struct ucred))` = 32. Every field is written, so no
    /// byte of it is uninitialized.
    #[repr(C)]
    struct CredMsg {
        cmsg_len: usize,
        cmsg_level: i32,
        cmsg_type: i32,
        pid: i32,
        uid: u32,
        gid: u32,
        pad: u32,
    }
    const _: () = assert!(std::mem::size_of::<CredMsg>() == 32);
    /// `CMSG_LEN(sizeof(struct ucred))`: the header and the 12-byte `ucred`.
    const CMSG_LEN_UCRED: usize = 16 + 12;
    const SOL_SOCKET: i32 = 1;
    const SCM_CREDENTIALS: i32 = 2;
    const MSG_NOSIGNAL: i32 = 0x4000;

    const AF_INET: i32 = 2;
    const AF_INET6: i32 = 10;
    /// `TIME_OK`, `ntp_gettime`'s "clock synchronized" state.
    const TIME_OK: i32 = 0;

    mod ffi {
        use super::{AddrInfo, Msghdr, Passwd, Timex};
        use std::ffi::c_char;

        unsafe extern "C" {
            pub fn getlogin() -> *const c_char;
            pub fn getpwuid(uid: u32) -> *mut Passwd;
            pub fn getuid() -> u32;
            pub fn geteuid() -> u32;
            pub fn kill(pid: i32, sig: i32) -> i32;
            pub fn sendmsg(fd: i32, msg: *const Msghdr, flags: i32) -> isize;
            pub fn gethostname(name: *mut c_char, len: usize) -> i32;
            pub fn adjtimex(buf: *mut Timex) -> i32;
            pub fn getaddrinfo(
                node: *const c_char,
                service: *const c_char,
                hints: *const AddrInfo,
                res: *mut *mut AddrInfo,
            ) -> i32;
            pub fn freeaddrinfo(res: *mut AddrInfo);
            pub fn gai_strerror(errcode: i32) -> *const c_char;
        }
    }

    pub fn getlogin() -> Option<Vec<u8>> {
        // SAFETY: `getlogin` takes no arguments and returns null or a
        // NUL-terminated string in static storage, valid until the next call
        // into the utmp family; it is copied before anything else runs.
        unsafe {
            let p = ffi::getlogin();
            if p.is_null() {
                return None;
            }
            Some(CStr::from_ptr(p).to_bytes().to_vec())
        }
    }

    #[allow(
        clippy::unnecessary_wraps,
        reason = "the host twin answers None; one signature serves both"
    )]
    pub fn getuid() -> Option<u32> {
        // SAFETY: `getuid` has no preconditions and cannot fail.
        Some(unsafe { ffi::getuid() })
    }

    #[allow(
        clippy::unnecessary_wraps,
        reason = "the host twin answers None; one signature serves both"
    )]
    pub fn geteuid() -> Option<u32> {
        // SAFETY: `geteuid` has no preconditions and cannot fail.
        Some(unsafe { ffi::geteuid() })
    }

    /// `kill(pid, 0) == 0`: `pid` names a process (or, for 0 and negative
    /// values, a group) this process may signal. Signal 0 sends nothing.
    pub fn process_exists(pid: i32) -> bool {
        // SAFETY: signal 0 performs only the existence and permission
        // checks; no signal is delivered, whatever `pid` is.
        unsafe { ffi::kill(pid, 0) == 0 }
    }

    pub fn send_as(fd: i32, wire: &[u8], pid: i32) -> io::Result<usize> {
        let iov = Iovec {
            iov_base: wire.as_ptr(),
            iov_len: wire.len(),
        };
        let mut cred = CredMsg {
            cmsg_len: CMSG_LEN_UCRED,
            cmsg_level: SOL_SOCKET,
            cmsg_type: SCM_CREDENTIALS,
            pid,
            uid: 0,
            gid: 0,
            pad: 0,
        };
        let msg = Msghdr {
            msg_name: std::ptr::null_mut(),
            msg_namelen: 0,
            msg_iov: &raw const iov,
            msg_iovlen: 1,
            msg_control: (&raw mut cred).cast(),
            msg_controllen: std::mem::size_of::<CredMsg>(),
            msg_flags: 0,
        };
        // SAFETY: `msg` is a valid `struct msghdr` for this ABI; its one
        // iovec points at `wire`, readable for `wire.len()` bytes, and its
        // control buffer at `cred`, a fully initialized, 8-aligned
        // `CMSG_SPACE(sizeof(struct ucred))`; all three outlive the call,
        // which only reads them. A bad `fd` is an error return, not UB.
        let n = unsafe { ffi::sendmsg(fd, &raw const msg, MSG_NOSIGNAL) };
        usize::try_from(n).map_err(|_| io::Error::last_os_error())
    }

    pub fn getpwuid_name(uid: u32) -> Option<Vec<u8>> {
        // SAFETY: `getpwuid` returns null or a pointer to a `struct passwd`
        // in static storage whose `pw_name`, when non-null, is NUL-terminated;
        // the name is copied before any other passwd-family call can reuse
        // the storage.
        unsafe {
            let pw = ffi::getpwuid(uid);
            if pw.is_null() || (*pw).pw_name.is_null() {
                return None;
            }
            let name = CStr::from_ptr((*pw).pw_name).to_bytes();
            // Upstream: `pw && pw->pw_name && *pw->pw_name`.
            (!name.is_empty()).then(|| name.to_vec())
        }
    }

    pub fn gethostname() -> Option<Vec<u8>> {
        // HOST_NAME_MAX is 64 on Linux; util-linux sizes its buffer from
        // sysconf(_SC_HOST_NAME_MAX), which answers the same. The last byte
        // stays 0, so a truncated name is still terminated.
        let mut buf = [0u8; 65];
        let len = buf.len().saturating_sub(1);
        // SAFETY: the buffer is valid for `len` bytes, and `gethostname`
        // writes at most that many.
        let rc = unsafe { ffi::gethostname(buf.as_mut_ptr().cast(), len) };
        if rc != 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(len);
        Some(buf.get(..end)?.to_vec())
    }

    pub fn adjtimex_maxerror() -> Option<i64> {
        // `ntp_gettime` is `adjtimex` with `modes = 0`: it reads, and returns
        // the clock state.
        // SAFETY: `Timex` is plain data, for which all-zero is a valid value.
        let mut tx: Timex = unsafe { std::mem::zeroed() };
        // SAFETY: `tx` is a valid, writable `struct timex` of the kernel's
        // size, and `modes` is 0, so nothing is changed.
        let state = unsafe { ffi::adjtimex(&raw mut tx) };
        (state == TIME_OK).then_some(tx.maxerror)
    }

    pub fn resolve(node: &[u8], service: &[u8], socktype: i32) -> Result<SocketAddr, String> {
        let (Ok(node), Ok(service)) = (CString::new(node), CString::new(service)) else {
            // An argv word cannot hold a NUL, so this is unreachable; answer
            // as getaddrinfo would for a name it cannot find.
            return Err("Name or service not known".to_string());
        };
        let hints = AddrInfo {
            ai_flags: 0,
            ai_family: 0, // AF_UNSPEC
            ai_socktype: socktype,
            ai_protocol: 0,
            ai_addrlen: 0,
            ai_addr: std::ptr::null_mut(),
            ai_canonname: std::ptr::null_mut(),
            ai_next: std::ptr::null_mut(),
        };
        let mut res: *mut AddrInfo = std::ptr::null_mut();
        // SAFETY: both strings are NUL-terminated and outlive the call;
        // `hints` is a valid addrinfo; `res` is a valid place for the result.
        let rc = unsafe {
            ffi::getaddrinfo(
                node.as_ptr(),
                service.as_ptr(),
                &raw const hints,
                &raw mut res,
            )
        };
        if rc != 0 || res.is_null() {
            // SAFETY: `gai_strerror` returns a NUL-terminated static string.
            let text = unsafe { CStr::from_ptr(ffi::gai_strerror(rc)) };
            return Err(c_text(text));
        }
        // SAFETY: `res` is the non-null list getaddrinfo returned; its first
        // entry's `ai_addr` points at `ai_addrlen` bytes of a sockaddr of
        // family `ai_family`. It is read before the list is freed, once.
        let addr = unsafe {
            let first = &*res;
            let addr = sockaddr_to_std(first.ai_family, first.ai_addr, first.ai_addrlen);
            ffi::freeaddrinfo(res);
            addr
        };
        addr.ok_or_else(|| "Address family for hostname not supported".to_string())
    }

    /// A `sockaddr_in`/`sockaddr_in6` as std's type.
    ///
    /// # Safety
    ///
    /// `sa` must point at `len` readable bytes holding a sockaddr of `family`.
    unsafe fn sockaddr_to_std(family: i32, sa: *const u8, len: u32) -> Option<SocketAddr> {
        let len = usize::try_from(len).ok()?;
        if sa.is_null() {
            return None;
        }
        // SAFETY: the caller guarantees `len` readable bytes at `sa`.
        let b = unsafe { std::slice::from_raw_parts(sa, len) };
        let port = u16::from_be_bytes([*b.get(2)?, *b.get(3)?]);
        match family {
            AF_INET => {
                let ip: [u8; 4] = b.get(4..8)?.try_into().ok()?;
                Some(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(ip), port)))
            }
            AF_INET6 => {
                let flow = u32::from_ne_bytes(b.get(4..8)?.try_into().ok()?);
                let ip: [u8; 16] = b.get(8..24)?.try_into().ok()?;
                let scope = u32::from_ne_bytes(b.get(24..28)?.try_into().ok()?);
                Some(SocketAddr::V6(SocketAddrV6::new(
                    Ipv6Addr::from(ip),
                    port,
                    flow,
                    scope,
                )))
            }
            _ => None,
        }
    }

    /// A C string from libc as text. `gai_strerror`'s messages are ASCII in
    /// every libc this runs on; were one not, its bytes are shown escaped
    /// rather than replaced.
    fn c_text(s: &CStr) -> String {
        match s.to_str() {
            Ok(t) => t.to_string(),
            Err(_) => s.to_bytes().escape_ascii().to_string(),
        }
    }
}

#[cfg(not(unix))]
mod imp {
    //! The Windows host the unit tests run on has none of these; every answer
    //! is "unknown", which the callers already handle.
    use std::net::SocketAddr;

    pub fn getlogin() -> Option<Vec<u8>> {
        None
    }
    pub fn getuid() -> Option<u32> {
        None
    }
    pub fn getpwuid_name(_uid: u32) -> Option<Vec<u8>> {
        None
    }
    pub fn geteuid() -> Option<u32> {
        None
    }
    pub fn process_exists(_pid: i32) -> bool {
        false
    }
    pub fn gethostname() -> Option<Vec<u8>> {
        None
    }
    pub fn adjtimex_maxerror() -> Option<i64> {
        None
    }
    pub fn resolve(_node: &[u8], _service: &[u8], _socktype: i32) -> Result<SocketAddr, String> {
        Err("Name or service not known".to_string())
    }
}
