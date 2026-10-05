//! Names and addresses as the C library resolves them: `getaddrinfo`,
//! `getnameinfo`, `gethostbyname` and `getifaddrs`.
//!
//! Asked of the one library a program links (design-decisions §768), so a
//! utility is told what every C program on the same system is told -- the
//! hosts file and DNS in the library's order, the interfaces the library
//! lists. `hostname -f`, `-i`, `-a`, `-I` and `-A` are each one of these
//! calls in net-tools' `hostname.c`, and are each one here.
//!
//! Nothing allocates: this crate is `no_std` without `alloc`. A lookup's
//! results stay in the library's own list, freed when the owning value drops
//! (`AddrInfo`, `IfAddrs`), and a printable answer is copied into a fixed
//! [`HostText`] the size the C interface already bounds it to (`NI_MAXHOST`).
//!
//! The record layouts are glibc's on x86_64, which SlateOS's C library
//! shares; the tests hold every size and offset to `posix`'s declarations.

use core::ffi::{CStr, c_char, c_int, c_void};

/// IPv4.
pub const AF_INET: i32 = 2;
/// IPv6.
pub const AF_INET6: i32 = 10;
/// Datagram sockets: the socket type `hostname` asks `getaddrinfo` for.
pub const SOCK_DGRAM: i32 = 2;
/// `getaddrinfo`: also return the canonical name.
pub const AI_CANONNAME: i32 = 0x0002;
/// `getnameinfo`: the numeric form of the address, never a name.
pub const NI_NUMERICHOST: i32 = 0x01;
/// `getnameinfo`: fail rather than fall back to the numeric form.
pub const NI_NAMEREQD: i32 = 0x08;
/// `getaddrinfo`/`getnameinfo`: the name or address is not known.
pub const EAI_NONAME: i32 = -2;
/// The interface is up.
pub const IFF_UP: u32 = 1;
/// The interface is the loopback.
pub const IFF_LOOPBACK: u32 = 8;
/// The longest name `getnameinfo` writes, with its terminator.
pub const NI_MAXHOST: usize = 1025;
/// Room for the longest numeric IPv6 address and its terminator.
pub const INET6_ADDRSTRLEN: usize = 46;
/// `inet_ntop`: the address family is neither IPv4 nor IPv6.
pub const EAFNOSUPPORT: i32 = 97;

/// `struct sockaddr`.
#[repr(C)]
pub struct Sockaddr {
    sa_family: u16,
    sa_data: [u8; 14],
}

/// `struct addrinfo`.
#[repr(C)]
struct Addrinfo {
    ai_flags: c_int,
    ai_family: c_int,
    ai_socktype: c_int,
    ai_protocol: c_int,
    ai_addrlen: u32,
    ai_addr: *mut Sockaddr,
    ai_canonname: *mut c_char,
    ai_next: *mut Addrinfo,
}

/// `struct ifaddrs`.
#[repr(C)]
struct Ifaddrs {
    ifa_next: *mut Ifaddrs,
    ifa_name: *const c_char,
    ifa_flags: u32,
    ifa_addr: *const Sockaddr,
    ifa_netmask: *const Sockaddr,
    ifa_ifu: *const Sockaddr,
    ifa_data: *mut c_void,
}

/// `struct hostent`.
///
/// Only the library makes one -- `gethostbyname` hands back its own -- so a
/// host build, which has no library to ask, never names it outside the tests.
#[repr(C)]
#[cfg_attr(not(unix), allow(dead_code))]
struct Hostent {
    h_name: *const c_char,
    h_aliases: *const *const c_char,
    h_addrtype: c_int,
    h_length: c_int,
    h_addr_list: *const *const c_char,
}

#[cfg(unix)]
mod sys {
    use super::{Addrinfo, Hostent, Ifaddrs, Sockaddr, c_char, c_int, c_void};

    unsafe extern "C" {
        pub fn inet_ntop(
            af: c_int,
            src: *const c_void,
            dst: *mut c_char,
            size: u32,
        ) -> *const c_char;
        pub fn getaddrinfo(
            node: *const c_char,
            service: *const c_char,
            hints: *const Addrinfo,
            res: *mut *mut Addrinfo,
        ) -> c_int;
        pub fn freeaddrinfo(res: *mut Addrinfo);
        pub fn gai_strerror(code: c_int) -> *const c_char;
        pub fn getnameinfo(
            sa: *const Sockaddr,
            salen: u32,
            host: *mut c_char,
            hostlen: u32,
            serv: *mut c_char,
            servlen: u32,
            flags: c_int,
        ) -> c_int;
        pub fn gethostbyname(name: *const c_char) -> *const Hostent;
        pub fn __h_errno_location() -> *mut c_int;
        pub fn hstrerror(err: c_int) -> *const c_char;
        pub fn getifaddrs(ifap: *mut *mut Ifaddrs) -> c_int;
        pub fn freeifaddrs(ifa: *mut Ifaddrs);
    }
}

/// A host name or numeric address as `getnameinfo` writes it, kept in place.
#[derive(Clone, Copy)]
pub struct HostText {
    buf: [u8; NI_MAXHOST],
    len: usize,
}

impl HostText {
    /// The text, without its terminator.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or_default()
    }
}

/// `getnameinfo(sa, salen, host, hostlen, NULL, 0, flags)`, where `hostlen`
/// is at most [`NI_MAXHOST`]: a caller that gives the library less room is
/// told `EAI_OVERFLOW` for an answer that does not fit, as it would be in C.
#[cfg(unix)]
fn name_info(sa: *const Sockaddr, salen: u32, flags: i32, hostlen: usize) -> Result<HostText, i32> {
    let mut text = HostText {
        buf: [0; NI_MAXHOST],
        len: 0,
    };
    // At most `NI_MAXHOST`, a small constant; the cast cannot truncate.
    #[allow(clippy::cast_possible_truncation)]
    let hostlen = hostlen.min(NI_MAXHOST) as u32;
    // SAFETY: `sa` points at a socket address of `salen` bytes that the
    // library itself produced and still owns for the duration of the call;
    // `host` is our buffer of `hostlen` bytes, which the library writes at
    // most that many of, NUL included; no service is asked for.
    let rc = unsafe {
        sys::getnameinfo(
            sa,
            salen,
            text.buf.as_mut_ptr().cast::<c_char>(),
            hostlen,
            core::ptr::null_mut(),
            0,
            flags,
        )
    };
    if rc != 0 {
        return Err(rc);
    }
    text.len = text.buf.iter().position(|&b| b == 0).unwrap_or(NI_MAXHOST);
    Ok(text)
}

/// `gai_strerror(code)`: the library's words for a lookup failure.
#[must_use]
pub fn gai_strerror(code: i32) -> &'static CStr {
    #[cfg(unix)]
    {
        // SAFETY: `gai_strerror` returns a pointer to a NUL-terminated string
        // in static storage for every code, known or not.
        unsafe { CStr::from_ptr(sys::gai_strerror(code)) }
    }
    #[cfg(not(unix))]
    {
        let _ = code;
        c"Name resolution is not available on this host"
    }
}

/// The result list of one `getaddrinfo` call, freed on drop.
pub struct AddrInfo {
    head: *mut Addrinfo,
}

impl AddrInfo {
    /// `getaddrinfo(host, NULL, hints, &res)` with the hints `socktype` and
    /// `flags` and everything else zero.
    ///
    /// # Errors
    ///
    /// The `EAI_*` code `getaddrinfo` returned, for [`gai_strerror`].
    pub fn lookup(host: &CStr, socktype: i32, flags: i32) -> Result<Self, i32> {
        #[cfg(unix)]
        {
            let hints = Addrinfo {
                ai_flags: flags,
                ai_family: 0,
                ai_socktype: socktype,
                ai_protocol: 0,
                ai_addrlen: 0,
                ai_addr: core::ptr::null_mut(),
                ai_canonname: core::ptr::null_mut(),
                ai_next: core::ptr::null_mut(),
            };
            let mut head: *mut Addrinfo = core::ptr::null_mut();
            // SAFETY: `host` is a valid C string for the call, `hints` a
            // fully initialised record the library only reads, and `head`
            // a valid place for the library to store its list.
            let rc = unsafe {
                sys::getaddrinfo(
                    host.as_ptr(),
                    core::ptr::null(),
                    &raw const hints,
                    &raw mut head,
                )
            };
            if rc != 0 {
                return Err(rc);
            }
            Ok(Self { head })
        }
        #[cfg(not(unix))]
        {
            let _ = (host, socktype, flags);
            Err(EAI_NONAME)
        }
    }

    /// The first entry's `ai_canonname`, when the library gave one.
    #[must_use]
    pub fn canonical_name(&self) -> Option<&[u8]> {
        // SAFETY: `head` is the non-null list `getaddrinfo` returned and this
        // value owns until it drops; the canonical name, if set, is a
        // NUL-terminated string inside that list.
        unsafe {
            let first = self.head.as_ref()?;
            if first.ai_canonname.is_null() {
                return None;
            }
            Some(CStr::from_ptr(first.ai_canonname).to_bytes())
        }
    }

    /// Each entry's address, through `getnameinfo` with `flags` and a buffer
    /// of `hostlen` bytes (at most [`NI_MAXHOST`]), in list order; an `Err`
    /// is the `EAI_*` code for that entry.
    #[must_use]
    pub fn names(&self, flags: i32, hostlen: usize) -> AddrNames<'_> {
        AddrNames {
            next: self.head,
            flags,
            hostlen,
            _list: core::marker::PhantomData,
        }
    }
}

impl Drop for AddrInfo {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: `head` came from a successful `getaddrinfo` and is freed
        // exactly once, here.
        unsafe {
            sys::freeaddrinfo(self.head);
        }
    }
}

/// The addresses of an [`AddrInfo`] list, as text.
pub struct AddrNames<'a> {
    next: *mut Addrinfo,
    flags: i32,
    hostlen: usize,
    _list: core::marker::PhantomData<&'a AddrInfo>,
}

impl Iterator for AddrNames<'_> {
    type Item = Result<HostText, i32>;

    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: `next` is null or an entry of the list the borrowed
        // `AddrInfo` keeps alive.
        let entry = unsafe { self.next.as_ref()? };
        self.next = entry.ai_next;
        #[cfg(unix)]
        {
            Some(name_info(
                entry.ai_addr,
                entry.ai_addrlen,
                self.flags,
                self.hostlen,
            ))
        }
        #[cfg(not(unix))]
        {
            let _ = (self.flags, self.hostlen);
            Some(Err(EAI_NONAME))
        }
    }
}

/// `gethostbyname(name)`'s alias list, each passed to `each`.
///
/// # Errors
///
/// `h_errno` when the lookup failed, for [`hstrerror`].
pub fn host_aliases(name: &CStr, mut each: impl FnMut(&[u8])) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: `name` is a valid C string for the call. The result is null
        // or the library's static record, read before any other lookup.
        let host = unsafe { sys::gethostbyname(name.as_ptr()) };
        // SAFETY: as above; null is checked before anything is read.
        let Some(host) = (unsafe { host.as_ref() }) else {
            // SAFETY: `__h_errno_location` returns this thread's `h_errno`,
            // valid for the life of the thread.
            return Err(unsafe { *sys::__h_errno_location() });
        };
        let mut alias = host.h_aliases;
        // SAFETY: `h_aliases` is a NULL-terminated array of NUL-terminated
        // strings inside the static record; the walk stops at the NULL.
        unsafe {
            while !alias.is_null() && !(*alias).is_null() {
                each(CStr::from_ptr(*alias).to_bytes());
                alias = alias.add(1);
            }
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (name, &mut each);
        Err(1)
    }
}

/// `hstrerror(code)`: the library's words for an `h_errno`.
#[must_use]
pub fn hstrerror(code: i32) -> &'static CStr {
    #[cfg(unix)]
    {
        // SAFETY: `hstrerror` returns a static NUL-terminated string for
        // every code.
        unsafe { CStr::from_ptr(sys::hstrerror(code)) }
    }
    #[cfg(not(unix))]
    {
        let _ = code;
        c"Unknown host"
    }
}

/// `inet_ntop(af, src, dst, dst.len())`: the address in `src` -- four bytes
/// for [`AF_INET`], sixteen for [`AF_INET6`], in network order -- as text in
/// `dst`, which then holds the text and a NUL after it. Returns the text's
/// length.
///
/// The library's own formatter rather than one written here, because which run
/// of zeros becomes `::`, and when an IPv6 address ends in a dotted quad
/// instead, are the library's decisions: `w -i` prints what `inet_ntop`
/// prints, and a second formatter would be a second answer.
///
/// # Errors
///
/// * `ENOSPC` -- `dst` cannot hold the text and its NUL. A caller may rely on
///   this: `w` offers a buffer only as wide as its column, and an address that
///   does not fit is shown as the host name instead.
/// * [`EINVAL`](crate::EINVAL) -- `src` is shorter than `af`'s address.
/// * [`EAFNOSUPPORT`] -- `af` is neither family.
/// * [`ENOSYS`](crate::ENOSYS) -- built for a host with no C library of ours.
pub fn inet_ntop(af: i32, src: &[u8], dst: &mut [u8]) -> Result<usize, i32> {
    let need = match af {
        AF_INET => 4,
        AF_INET6 => 16,
        _ => return Err(EAFNOSUPPORT),
    };
    if src.len() < need {
        return Err(crate::EINVAL);
    }
    #[cfg(unix)]
    {
        // `socklen_t` is 32 bits. A larger buffer is offered as its first
        // 4 GiB, which holds any address many times over.
        let size = u32::try_from(dst.len()).unwrap_or(u32::MAX);
        // SAFETY: `src` holds at least the address `af` names (checked above)
        // and is only read; `dst` is writable for `size` bytes, no more than
        // its length, and the library writes at most that many, NUL included.
        let text = unsafe {
            sys::inet_ntop(
                af,
                src.as_ptr().cast::<c_void>(),
                dst.as_mut_ptr().cast::<c_char>(),
                size,
            )
        };
        if text.is_null() {
            return Err(crate::last_errno());
        }
        Ok(dst.iter().position(|&b| b == 0).unwrap_or(dst.len()))
    }
    #[cfg(not(unix))]
    {
        let _ = dst;
        Err(crate::ENOSYS)
    }
}

/// The library's interface list (`getifaddrs`), freed on drop.
pub struct IfAddrs {
    head: *mut Ifaddrs,
}

impl IfAddrs {
    /// `getifaddrs(&list)`.
    ///
    /// # Errors
    ///
    /// The `errno` it failed with, and [`crate::ENOSYS`] where there is no C
    /// library.
    pub fn get() -> Result<Self, i32> {
        #[cfg(unix)]
        {
            let mut head: *mut Ifaddrs = core::ptr::null_mut();
            // SAFETY: `head` is a valid place for the library to store its
            // list head.
            if unsafe { sys::getifaddrs(&raw mut head) } != 0 {
                return Err(super::last_errno());
            }
            Ok(Self { head })
        }
        #[cfg(not(unix))]
        {
            Err(crate::ENOSYS)
        }
    }

    /// The entries, in the library's order.
    #[must_use]
    pub fn iter(&self) -> IfAddrIter<'_> {
        IfAddrIter {
            next: self.head,
            _list: core::marker::PhantomData,
        }
    }
}

impl<'a> IntoIterator for &'a IfAddrs {
    type Item = IfAddr<'a>;
    type IntoIter = IfAddrIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl Drop for IfAddrs {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: `head` came from a successful `getifaddrs` (or is null,
        // which `freeifaddrs` accepts) and is freed exactly once, here.
        unsafe {
            sys::freeifaddrs(self.head);
        }
    }
}

/// The entries of an [`IfAddrs`] list.
pub struct IfAddrIter<'a> {
    next: *mut Ifaddrs,
    _list: core::marker::PhantomData<&'a IfAddrs>,
}

impl<'a> Iterator for IfAddrIter<'a> {
    type Item = IfAddr<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: `next` is null or an entry of the list the borrowed
        // `IfAddrs` keeps alive for `'a`.
        let entry: &'a Ifaddrs = unsafe { self.next.as_ref()? };
        self.next = entry.ifa_next;
        Some(IfAddr { entry })
    }
}

/// One interface entry.
pub struct IfAddr<'a> {
    entry: &'a Ifaddrs,
}

impl IfAddr<'_> {
    /// `ifa_flags`.
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.entry.ifa_flags
    }

    /// The address family of `ifa_addr`, or `None` when the entry has no
    /// address.
    #[must_use]
    pub fn family(&self) -> Option<i32> {
        // SAFETY: `ifa_addr` is null or points at a socket address inside
        // the list this entry borrows from.
        unsafe { self.entry.ifa_addr.as_ref() }.map(|sa| i32::from(sa.sa_family))
    }

    /// Whether this is an IPv6 link-local unicast (`fe80::/10`) or link-local
    /// multicast (`ffx2::/16`) address: glibc's `IN6_IS_ADDR_LINKLOCAL` and
    /// `IN6_IS_ADDR_MC_LINKLOCAL`.
    #[must_use]
    pub fn is_ipv6_link_local(&self) -> bool {
        if self.family() != Some(AF_INET6) {
            return false;
        }
        // `sin6_addr` sits 8 bytes into `struct sockaddr_in6`: family 2,
        // port 2, flowinfo 4.
        // SAFETY: an `AF_INET6` address is a `struct sockaddr_in6`, 28 bytes,
        // so bytes 8 and 9 are inside it.
        let (a0, a1) = unsafe {
            let base = self.entry.ifa_addr.cast::<u8>();
            (*base.add(8), *base.add(9))
        };
        (a0 == 0xfe && (a1 & 0xc0) == 0x80) || (a0 == 0xff && (a1 & 0x0f) == 0x02)
    }

    /// `getnameinfo` on the address, with `flags`, into a buffer of
    /// [`NI_MAXHOST`] bytes -- the size net-tools' `hostname -I` and `-A` give
    /// it.
    ///
    /// # Errors
    ///
    /// The `EAI_*` code it returned.
    pub fn name_info(&self, flags: i32) -> Result<HostText, i32> {
        let salen: u32 = match self.family() {
            Some(AF_INET) => 16,
            Some(AF_INET6) => 28,
            _ => return Err(EAI_NONAME),
        };
        #[cfg(unix)]
        {
            name_info(self.entry.ifa_addr, salen, flags, NI_MAXHOST)
        }
        #[cfg(not(unix))]
        {
            let _ = (salen, flags);
            Err(EAI_NONAME)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[test]
    fn the_records_are_the_c_librarys_field_for_field() {
        use posix::socket as p;
        assert_eq!(size_of::<Addrinfo>(), size_of::<p::Addrinfo>());
        assert_eq!(
            offset_of!(Addrinfo, ai_addrlen),
            offset_of!(p::Addrinfo, ai_addrlen)
        );
        assert_eq!(
            offset_of!(Addrinfo, ai_addr),
            offset_of!(p::Addrinfo, ai_addr)
        );
        assert_eq!(
            offset_of!(Addrinfo, ai_canonname),
            offset_of!(p::Addrinfo, ai_canonname)
        );
        assert_eq!(
            offset_of!(Addrinfo, ai_next),
            offset_of!(p::Addrinfo, ai_next)
        );
        assert_eq!(size_of::<Ifaddrs>(), size_of::<p::Ifaddrs>());
        assert_eq!(
            offset_of!(Ifaddrs, ifa_flags),
            offset_of!(p::Ifaddrs, ifa_flags)
        );
        assert_eq!(
            offset_of!(Ifaddrs, ifa_addr),
            offset_of!(p::Ifaddrs, ifa_addr)
        );
        assert_eq!(
            offset_of!(Ifaddrs, ifa_data),
            offset_of!(p::Ifaddrs, ifa_data)
        );
        assert_eq!(size_of::<Hostent>(), size_of::<p::Hostent>());
        assert_eq!(
            offset_of!(Hostent, h_aliases),
            offset_of!(p::Hostent, h_aliases)
        );
        assert_eq!(size_of::<Sockaddr>(), size_of::<p::Sockaddr>());
    }

    #[test]
    fn the_constants_are_the_c_librarys() {
        assert_eq!(AF_INET, posix::socket::AF_INET);
        assert_eq!(AF_INET6, posix::socket::AF_INET6);
        assert_eq!(SOCK_DGRAM, posix::socket::SOCK_DGRAM);
        assert_eq!(AI_CANONNAME, posix::socket::AI_CANONNAME);
        assert_eq!(EAI_NONAME, posix::socket::EAI_NONAME);
        assert_eq!(IFF_UP, posix::socket::IFF_UP);
        assert_eq!(IFF_LOOPBACK, posix::socket::IFF_LOOPBACK);
        assert_eq!(NI_NUMERICHOST, posix::gai::NI_NUMERICHOST);
        assert_eq!(NI_NAMEREQD, posix::gai::NI_NAMEREQD);
        assert_eq!(EAFNOSUPPORT, posix::errno::EAFNOSUPPORT);
    }

    /// The refusals made before the library is asked, on every build.
    #[test]
    fn inet_ntop_refuses_an_unknown_family_and_a_short_address() {
        let mut buf = [0u8; INET6_ADDRSTRLEN];
        assert_eq!(inet_ntop(99, &[0; 16], &mut buf), Err(EAFNOSUPPORT));
        assert_eq!(
            inet_ntop(AF_INET, &[127, 0, 1], &mut buf),
            Err(crate::EINVAL)
        );
        assert_eq!(inet_ntop(AF_INET6, &[0; 15], &mut buf), Err(crate::EINVAL));
    }

    /// The library's text, and its `ENOSPC` for a buffer one byte short --
    /// which `w -i` turns into "show the host name instead".
    #[cfg(unix)]
    #[test]
    fn inet_ntop_is_the_librarys() {
        let check = |af: i32, src: &[u8], want: &[u8]| {
            let mut buf = [0u8; INET6_ADDRSTRLEN];
            let len = inet_ntop(af, src, &mut buf);
            assert_eq!(len, Ok(want.len()));
            assert_eq!(buf.get(..want.len()), Some(want));
            assert_eq!(buf.get(want.len()), Some(&0), "the text is NUL-terminated");
        };
        check(AF_INET, &[127, 0, 0, 1], b"127.0.0.1");
        check(AF_INET, &[255, 255, 255, 255], b"255.255.255.255");
        check(
            AF_INET6,
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            b"::1",
        );
        let doc = [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        check(AF_INET6, &doc, b"2001:db8::1");
        let mapped = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 1, 2, 3, 4];
        check(AF_INET6, &mapped, b"::ffff:1.2.3.4");
        let compatible = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4];
        check(AF_INET6, &compatible, b"::1.2.3.4");

        // "127.0.0.1" is nine bytes; with its NUL it needs ten.
        let mut short = [0u8; 9];
        assert_eq!(inet_ntop(AF_INET, &[127, 0, 0, 1], &mut short), Err(28));
        let mut exact = [0u8; 10];
        assert_eq!(inet_ntop(AF_INET, &[127, 0, 0, 1], &mut exact), Ok(9));
    }

    #[cfg(not(unix))]
    #[test]
    fn inet_ntop_has_no_library_on_the_host() {
        let mut buf = [0u8; INET6_ADDRSTRLEN];
        assert_eq!(
            inet_ntop(AF_INET, &[127, 0, 0, 1], &mut buf),
            Err(crate::ENOSYS)
        );
    }

    #[test]
    fn host_text_is_its_bytes_up_to_the_length() {
        let mut t = HostText {
            buf: [0; NI_MAXHOST],
            len: 3,
        };
        if let Some(head) = t.buf.get_mut(..4) {
            head.copy_from_slice(b"abcd");
        }
        assert_eq!(t.as_bytes(), b"abc");
    }

    /// A `struct sockaddr_in6`'s bytes, aligned as the library aligns one.
    #[repr(C, align(4))]
    struct Sin6([u8; 28]);

    /// An interface entry whose address is `addr`, and nothing else.
    fn entry(addr: *const Sockaddr) -> Ifaddrs {
        Ifaddrs {
            ifa_next: core::ptr::null_mut(),
            ifa_name: core::ptr::null(),
            ifa_flags: IFF_UP,
            ifa_addr: addr,
            ifa_netmask: core::ptr::null(),
            ifa_ifu: core::ptr::null(),
            ifa_data: core::ptr::null_mut(),
        }
    }

    /// Whether glibc's two link-local tests would pass over an IPv6 address
    /// starting with `a0`, `a1`.
    fn link_local(a0: u8, a1: u8) -> bool {
        let mut sin6 = Sin6([0; 28]);
        // AF_INET6 is 10, little-endian in `sin6_family`.
        sin6.0[0] = 10;
        sin6.0[8] = a0;
        sin6.0[9] = a1;
        let e = entry((&raw const sin6).cast::<Sockaddr>());
        IfAddr { entry: &e }.is_ipv6_link_local()
    }

    #[test]
    fn the_link_local_test_is_glibcs() {
        assert!(link_local(0xfe, 0x80), "fe80::/10");
        assert!(link_local(0xfe, 0xbf), "fe80::/10's top");
        assert!(!link_local(0xfe, 0xc0), "fec0:: is site-local");
        assert!(link_local(0xff, 0x02), "ff02:: link-local multicast");
        assert!(
            link_local(0xff, 0x12),
            "ff12:: transient link-local multicast"
        );
        assert!(!link_local(0xff, 0x05), "ff05:: site-local multicast");
        assert!(!link_local(0x20, 0x01), "2001:: global");
    }

    #[test]
    fn an_entry_without_an_address_has_no_family_and_no_name() {
        let e = entry(core::ptr::null());
        let i = IfAddr { entry: &e };
        assert_eq!(i.family(), None);
        assert!(!i.is_ipv6_link_local());
        assert_eq!(i.name_info(NI_NUMERICHOST).err(), Some(EAI_NONAME));
    }

    #[test]
    fn an_ipv4_address_is_not_link_local() {
        let mut sin = Sin6([0; 28]);
        sin.0[0] = 2;
        sin.0[4] = 0xfe;
        sin.0[5] = 0x80;
        let e = entry((&raw const sin).cast::<Sockaddr>());
        let i = IfAddr { entry: &e };
        assert_eq!(i.family(), Some(AF_INET));
        assert!(!i.is_ipv6_link_local());
    }

    /// `localhost` is in every hosts file: the library resolves it, calls it
    /// that canonically and gives a loopback address for it.
    #[cfg(unix)]
    #[test]
    fn localhost_resolves_through_the_library() {
        let found = AddrInfo::lookup(c"localhost", SOCK_DGRAM, AI_CANONNAME);
        assert!(
            found.is_ok(),
            "getaddrinfo(localhost): {:?}",
            found.as_ref().err()
        );
        if let Ok(list) = found {
            assert_eq!(list.canonical_name(), Some(&b"localhost"[..]));
            assert!(
                list.names(NI_NUMERICHOST, INET6_ADDRSTRLEN)
                    .any(|n| matches!(
                        n,
                        Ok(t) if t.as_bytes() == b"127.0.0.1" || t.as_bytes() == b"::1"
                    ))
            );
        }
    }

    /// Every system has a loopback interface, and the library lists it with
    /// its address.
    #[cfg(unix)]
    #[test]
    fn the_interface_list_holds_the_loopback() {
        let list = IfAddrs::get();
        assert!(list.is_ok(), "getifaddrs: {:?}", list.as_ref().err());
        if let Ok(list) = list {
            assert!(list.iter().any(|i| {
                i.flags() & IFF_LOOPBACK != 0
                    && i.family() == Some(AF_INET)
                    && matches!(i.name_info(NI_NUMERICHOST), Ok(t) if t.as_bytes() == b"127.0.0.1")
            }));
        }
    }

    /// The library's words, not ours.
    #[cfg(unix)]
    #[test]
    fn the_failure_words_are_the_librarys() {
        assert_eq!(
            gai_strerror(EAI_NONAME).to_bytes(),
            b"Name or service not known"
        );
        assert_eq!(hstrerror(1).to_bytes(), b"Unknown host");
    }
}
