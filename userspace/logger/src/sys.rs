//! The libc calls util-linux's `logger` makes that std does not wrap, reached
//! through the C ABI as the one-libc rule requires (design-decisions §768):
//! `getlogin`, `getpwuid`, `getuid`/`geteuid`, `gethostname`, `ntp_gettime`
//! (as `adjtimex`), and name resolution for `-n`.
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

    const AF_INET: i32 = 2;
    const AF_INET6: i32 = 10;
    /// `TIME_OK`, `ntp_gettime`'s "clock synchronized" state.
    const TIME_OK: i32 = 0;

    mod ffi {
        use super::{AddrInfo, Passwd, Timex};
        use std::ffi::c_char;

        unsafe extern "C" {
            pub fn getlogin() -> *const c_char;
            pub fn getpwuid(uid: u32) -> *mut Passwd;
            pub fn getuid() -> u32;
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
