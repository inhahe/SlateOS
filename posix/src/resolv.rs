// Offsets in this file index DNS messages and names whose lengths are
// bounded by the protocol -- labels of at most 63 bytes, names of at most
// 255, headers of 12, messages of at most 64 KiB -- and every slice access
// that could miss goes through `get`/`get_mut`; the few fixed header offsets
// index a buffer checked to hold a header.  clippy cannot see across those
// checks.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
//! `<resolv.h>` — the DNS resolver: glibc 2.39's interface and name rules
//! over musl's transport.
//!
//! `getaddrinfo` asks the kernel's network stack for addresses
//! (`SYS_DNS_RESOLVE`); everything else a program wants from DNS -- mail
//! exchangers, service records, text records, any record type at all --
//! goes through these calls, which speak DNS to the nameservers in
//! `/etc/resolv.conf` over UDP and, for answers too big for a datagram, TCP.
//! Until 2026-09-26 `res_query`, `res_search`, `res_mkquery` and `res_send`
//! checked their arguments and returned `ENOSYS`
//! (`known-issues.md` → `B-D-RES-QUERY-WAS-ENOSYS`).
//!
//! - **Configuration** is glibc's `resolv.conf`: `nameserver` (IPv4, up to
//!   three), `domain` and `search`, and `options ndots: timeout: attempts:
//!   rotate use-vc no-tld-query trust-ad`; the `LOCALDOMAIN` and `RES_OPTIONS`
//!   environment variables override it, and with no domain the host name's
//!   own domain is searched.  No nameserver means the local one, 127.0.0.1.
//!   It lands in `_res` (`__res_state()`), where a program may change it
//!   before querying, as glibc lets it.
//! - **Names** are converted as glibc's `ns_name_*` functions convert them:
//!   `\.` and `\DDD` escapes both ways, labels of up to 63 bytes, names of up
//!   to 255, compression pointers followed -- forward ones included -- and
//!   written by `dn_comp` against its table of earlier names.
//! - **Queries** go to every nameserver at once and are sent again at
//!   intervals until the timeout, musl's way; the first acceptable answer
//!   wins.  A server's `SERVFAIL`, `NOTIMP` or `REFUSED` is waited past, as
//!   glibc moves on to the next server.  A truncated answer is asked again
//!   over TCP.
//! - **`res_query`** turns the answer's code into `h_errno` as glibc does,
//!   and **`res_search`** is glibc's search: `ndots`, the search list, the
//!   trailing dot, `RES_DEFNAMES`/`RES_DNSRCH`/`RES_NOTLDQUERY`.
//!
//! Not done: IPv6 nameservers (read, and skipped), `sortlist`, EDNS0,
//! `HOSTALIASES`, DNSSEC.  `_res` is one per process, as in musl; glibc's is
//! one per thread.

use crate::errno;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum DNS name length, as text.
pub const MAXDNAME: usize = 1025;
/// Maximum compressed DNS name length in a packet.
pub const MAXCDNAME: usize = 255;
/// Maximum DNS label length.
pub const MAXLABEL: usize = 63;

/// DNS header size.
pub const HFIXEDSZ: usize = 12;
/// DNS question fixed size (QTYPE + QCLASS).
pub const QFIXEDSZ: usize = 4;
/// DNS resource record fixed size.
pub const RRFIXEDSZ: usize = 10;
/// The largest UDP answer a plain query expects.
pub const PACKETSZ: usize = 512;

/// DNS class: Internet.
pub const C_IN: i32 = 1;
/// DNS class: Chaos.
pub const C_CH: i32 = 3;
/// DNS class: Any.
pub const C_ANY: i32 = 255;

/// DNS type: A (IPv4 address).
pub const T_A: i32 = 1;
/// DNS type: NS (name server).
pub const T_NS: i32 = 2;
/// DNS type: CNAME (canonical name).
pub const T_CNAME: i32 = 5;
/// DNS type: SOA (start of authority).
pub const T_SOA: i32 = 6;
/// DNS type: PTR (pointer).
pub const T_PTR: i32 = 12;
/// DNS type: MX (mail exchange).
pub const T_MX: i32 = 15;
/// DNS type: TXT (text).
pub const T_TXT: i32 = 16;
/// DNS type: AAAA (IPv6 address).
pub const T_AAAA: i32 = 28;
/// DNS type: SRV (service locator).
pub const T_SRV: i32 = 33;
/// DNS type: NULL (the completion record of a `NS_NOTIFY_OP`).
pub const T_NULL: i32 = 10;
/// DNS type: ANY (any type).
pub const T_ANY: i32 = 255;

/// DNS operation: Standard query.
pub const QUERY: i32 = 0;
/// DNS operation: Inverse query -- which glibc no longer builds.
pub const IQUERY: i32 = 1;
/// DNS operation: zone change notification.
pub const NS_NOTIFY_OP: i32 = 4;

/// Response codes.
const NOERROR: u8 = 0;
/// Only the tests name it: it is `NO_RECOVERY`, like every code but the
/// three `judge` singles out.
#[cfg(test)]
const FORMERR: u8 = 1;
const SERVFAIL: u8 = 2;
const NXDOMAIN: u8 = 3;
const NOTIMP: u8 = 4;
const REFUSED: u8 = 5;

/// Nameservers `_res` holds.
pub const MAXNS: usize = 3;
/// Search-list domains `_res` holds.
pub const MAXDNSRCH: usize = 6;
/// `sortlist` entries `_res` holds.
pub const MAXRESOLVSORT: usize = 10;
/// Default per-attempt timeout, seconds.
pub const RES_TIMEOUT: i32 = 5;
/// Default attempts.
pub const RES_DFLRETRY: i32 = 2;
/// The largest `ndots`.
pub const RES_MAXNDOTS: i32 = 15;
/// The largest `timeout`.
pub const RES_MAXRETRANS: i32 = 30;
/// The largest `attempts`.
pub const RES_MAXRETRY: i32 = 5;

/// `_res.options`: the state has been initialised.
pub const RES_INIT: u64 = 0x0000_0001;
/// Debug output (accepted, unused).
pub const RES_DEBUG: u64 = 0x0000_0002;
/// Use TCP ("virtual circuit") for every query.
pub const RES_USEVC: u64 = 0x0000_0008;
/// Ignore truncation (accepted, unused).
pub const RES_IGNTC: u64 = 0x0000_0020;
/// Ask for recursion (the RD bit).
pub const RES_RECURSE: u64 = 0x0000_0040;
/// Search the default domain for a name with no dots.
pub const RES_DEFNAMES: u64 = 0x0000_0080;
/// Search the search list for a name with dots.
pub const RES_DNSRCH: u64 = 0x0000_0200;
/// Rotate among the nameservers.
pub const RES_ROTATE: u64 = 0x0000_4000;
/// Never query a dot-free name as it stands.
pub const RES_NOTLDQUERY: u64 = 0x0010_0000;
/// Set the AD bit in queries.
pub const RES_TRUSTAD: u64 = 0x0400_0000;
/// The options a fresh state starts with.
pub const RES_DEFAULT: u64 = RES_RECURSE | RES_DEFNAMES | RES_DNSRCH;

/// `h_errno` for an error that is not the resolver's (see `errno`).
pub const NETDB_INTERNAL: i32 = -1;

// ---------------------------------------------------------------------------
// _res
// ---------------------------------------------------------------------------

/// `struct __res_state` -- glibc's and musl's layout, 568 bytes, which is
/// what `_res` (`(*__res_state())`) names in a C program.
#[repr(C)]
pub struct ResState {
    /// Per-attempt timeout, seconds.
    pub retrans: i32,
    /// Attempts.
    pub retry: i32,
    /// `RES_*` flags.
    pub options: u64,
    /// Nameservers in `nsaddr_list`.
    pub nscount: i32,
    /// The nameservers.
    pub nsaddr_list: [crate::socket::SockaddrIn; MAXNS],
    /// The last query id (unused here).
    pub id: u16,
    /// The search list: pointers into `defdname`, NULL-terminated.
    pub dnsrch: [*mut u8; MAXDNSRCH + 1],
    /// The search list's strings, each NUL-terminated.
    pub defdname: [u8; 256],
    /// Debug filter (unused).
    pub pfcode: u64,
    /// `ndots:4, nsort:4, ipv6_unavail:1` -- C bit-fields, from bit 0.
    pub bits: u32,
    /// `sortlist` (unused).
    pub sort_list: [[u32; 2]; MAXRESOLVSORT],
    /// Hooks (unused).
    pub qhook: *mut u8,
    /// Hooks (unused).
    pub rhook: *mut u8,
    /// The state's own `h_errno` (glibc keeps it too).
    pub res_h_errno: i32,
    /// Unused.
    pub _vcsock: i32,
    /// Unused.
    pub _flags: u32,
    /// glibc's `_u` union (unused).
    pub _u: [u64; 7],
}

const _: () = {
    assert!(size_of::<ResState>() == 568);
    assert!(core::mem::offset_of!(ResState, nsaddr_list) == 20);
    assert!(core::mem::offset_of!(ResState, dnsrch) == 72);
    assert!(core::mem::offset_of!(ResState, defdname) == 128);
    assert!(core::mem::offset_of!(ResState, bits) == 392);
    assert!(core::mem::offset_of!(ResState, res_h_errno) == 496);
};

impl ResState {
    const ZERO: Self = Self {
        retrans: 0,
        retry: 0,
        options: 0,
        nscount: 0,
        nsaddr_list: [SIN_ZERO; MAXNS],
        id: 0,
        dnsrch: [core::ptr::null_mut(); MAXDNSRCH + 1],
        defdname: [0; 256],
        pfcode: 0,
        bits: 0,
        sort_list: [[0; 2]; MAXRESOLVSORT],
        qhook: core::ptr::null_mut(),
        rhook: core::ptr::null_mut(),
        res_h_errno: 0,
        _vcsock: -1,
        _flags: 0,
        _u: [0; 7],
    };

    fn ndots(&self) -> usize {
        (self.bits & 0xf) as usize
    }
}

/// An all-zero `sockaddr_in`.
const SIN_ZERO: crate::socket::SockaddrIn = crate::socket::SockaddrIn {
    sin_family: 0,
    sin_port: 0,
    sin_addr: crate::socket::InAddr { s_addr: 0 },
    sin_zero: [0; 8],
};

crate::perprocess::process_global! {
    /// The process's resolver state.  Per-thread on the host, so tests do
    /// not configure each other's.
    fn res_storage() -> ResState = ResState::ZERO;
}

/// `__res_state` -- `_res`, the resolver state, which C reaches through
/// `#define _res (*__res_state())`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __res_state() -> *mut ResState {
    res_storage()
}

/// The state, initialised if nothing has initialised it yet -- glibc's
/// `__resolv_context_get`.
fn state() -> &'static mut ResState {
    // SAFETY: the process's (host: the thread's) resolver state; the
    // resolver is not reentrant against itself, as in glibc.
    let st = unsafe { &mut *res_storage() };
    if st.options & RES_INIT == 0 {
        load_state(st);
    }
    st
}

// ---------------------------------------------------------------------------
// resolv.conf
// ---------------------------------------------------------------------------

/// What `resolv.conf` (and the environment) said.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Conf {
    /// IPv4 nameservers, network byte order.
    ns: [[u8; 4]; MAXNS],
    nns: usize,
    /// The search list, as its domains, in order.
    search: [[u8; 64]; MAXDNSRCH],
    search_len: [usize; MAXDNSRCH],
    nsearch: usize,
    ndots: i32,
    timeout: i32,
    attempts: i32,
    options: u64,
}

impl Conf {
    const DEFAULT: Self = Self {
        ns: [[0; 4]; MAXNS],
        nns: 0,
        search: [[0; 64]; MAXDNSRCH],
        search_len: [0; MAXDNSRCH],
        nsearch: 0,
        ndots: 1,
        timeout: RES_TIMEOUT,
        attempts: RES_DFLRETRY,
        options: RES_DEFAULT,
    };

    /// Replace the search list with the blank-separated domains in `list`
    /// (a `search` line, or `LOCALDOMAIN`).
    fn set_search(&mut self, list: &[u8]) {
        self.nsearch = 0;
        for word in list
            .split(|&c| c == b' ' || c == b'\t' || c == b'\r' || c == b'\n')
            .filter(|w| !w.is_empty())
        {
            if self.nsearch == MAXDNSRCH {
                break;
            }
            let Some(slot) = self.search.get_mut(self.nsearch) else {
                break;
            };
            // A domain that does not fit a search slot cannot be searched.
            let Some(dst) = slot.get_mut(..word.len()) else {
                continue;
            };
            dst.copy_from_slice(word);
            if let Some(len) = self.search_len.get_mut(self.nsearch) {
                *len = word.len();
            }
            self.nsearch += 1;
        }
    }

    /// Apply an `options` line's words (or `RES_OPTIONS`).
    fn set_options(&mut self, words: &[u8]) {
        for w in words
            .split(|&c| c == b' ' || c == b'\t' || c == b'\r' || c == b'\n')
            .filter(|w| !w.is_empty())
        {
            let number = |prefix: &[u8]| -> Option<i32> {
                let digits = w.strip_prefix(prefix)?;
                let mut n: i32 = 0;
                for &d in digits {
                    if !d.is_ascii_digit() {
                        break;
                    }
                    n = n.saturating_mul(10).saturating_add(i32::from(d - b'0'));
                }
                digits.first().filter(|d| d.is_ascii_digit())?;
                Some(n)
            };
            if let Some(n) = number(b"ndots:") {
                self.ndots = n.min(RES_MAXNDOTS);
            } else if let Some(n) = number(b"timeout:") {
                self.timeout = n.clamp(1, RES_MAXRETRANS);
            } else if let Some(n) = number(b"attempts:") {
                self.attempts = n.clamp(1, RES_MAXRETRY);
            } else if w == b"rotate" {
                self.options |= RES_ROTATE;
            } else if w == b"use-vc" {
                self.options |= RES_USEVC;
            } else if w == b"no-tld-query" {
                self.options |= RES_NOTLDQUERY;
            } else if w == b"trust-ad" {
                self.options |= RES_TRUSTAD;
            }
            // debug, edns0, single-request, inet6 and the rest: accepted
            // and ignored, as unknown options are in glibc.
        }
    }
}

/// Parse `resolv.conf`'s text.  `host_domain` is the domain part of the
/// host name, the default search list when the file names none.
fn parse_conf(text: &[u8], host_domain: &[u8]) -> Conf {
    let mut c = Conf::DEFAULT;
    let mut saw_search = false;
    for line in text.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        // A comment starts with `;` or `#` in the first column.
        if matches!(line.first(), Some(b';' | b'#') | None) {
            continue;
        }
        let key_end = line
            .iter()
            .position(|&b| b == b' ' || b == b'\t')
            .unwrap_or(line.len());
        let (key, rest) = line.split_at(key_end);
        let rest = rest.trim_ascii();
        match key {
            b"nameserver" => {
                let addr = rest
                    .split(|&b| b == b' ' || b == b'\t')
                    .next()
                    .unwrap_or(&[]);
                if c.nns < MAXNS {
                    if let (Some(ip), Some(slot)) = (parse_ipv4(addr), c.ns.get_mut(c.nns)) {
                        *slot = ip;
                        c.nns += 1;
                    }
                }
            }
            b"domain" => {
                // `domain` names the one domain to search.
                let d = rest
                    .split(|&b| b == b' ' || b == b'\t')
                    .next()
                    .unwrap_or(&[]);
                c.set_search(d);
                saw_search = true;
            }
            b"search" => {
                c.set_search(rest);
                saw_search = true;
            }
            b"options" => c.set_options(rest),
            _ => {}
        }
    }
    if !saw_search && !host_domain.is_empty() {
        c.set_search(host_domain);
    }
    c
}

/// A dotted-quad IPv4 address, or `None` (IPv6 and anything else).
fn parse_ipv4(s: &[u8]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = s.split(|&b| b == b'.');
    for slot in &mut out {
        let p = parts.next()?;
        if p.is_empty() || p.len() > 3 || !p.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let v = p.iter().fold(0u32, |a, &d| a * 10 + u32::from(d - b'0'));
        *slot = u8::try_from(v).ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// Read a whole small file into `buf`; the bytes read.
fn read_file(path: &[u8], buf: &mut [u8]) -> Option<usize> {
    let fd = crate::file::open(
        path.as_ptr(),
        crate::fcntl::O_RDONLY | crate::fcntl::O_CLOEXEC,
        0,
    );
    if fd < 0 {
        return None;
    }
    let mut n = 0usize;
    while let Some(rest) = buf.get_mut(n..) {
        if rest.is_empty() {
            break;
        }
        let r = crate::file::read(fd, rest.as_mut_ptr(), rest.len());
        let Ok(r) = usize::try_from(r) else {
            break;
        };
        if r == 0 {
            break;
        }
        n += r;
    }
    // A read-only descriptor's close cannot lose anything.
    let _ = crate::file::close(fd);
    Some(n)
}

/// An environment variable's bytes.
fn env(name: &[u8]) -> Option<&'static [u8]> {
    // SAFETY: `name` is NUL-terminated; the value is a NUL-terminated
    // string that lives as long as the environment.
    unsafe {
        let v = crate::environ::getenv(name.as_ptr());
        if v.is_null() {
            return None;
        }
        Some(core::slice::from_raw_parts(v, crate::string::strlen(v)))
    }
}

/// Load `resolv.conf` and the environment into `st`, glibc's `res_init`.
fn load_state(st: &mut ResState) {
    let mut text = [0u8; 4096];
    let n = read_file(b"/etc/resolv.conf\0", &mut text).unwrap_or(0);
    let mut host = [0u8; 256];
    let host_domain = if crate::unistd::gethostname(host.as_mut_ptr(), host.len()) == 0 {
        let len = host.iter().position(|&b| b == 0).unwrap_or(host.len());
        let name = host.get(..len).unwrap_or(&[]);
        name.iter()
            .position(|&b| b == b'.')
            .and_then(|dot| name.get(dot + 1..))
            .unwrap_or(&[])
    } else {
        &[]
    };
    let mut c = parse_conf(text.get(..n).unwrap_or(&[]), host_domain);
    if let Some(d) = env(b"LOCALDOMAIN\0") {
        c.set_search(d);
    }
    if let Some(o) = env(b"RES_OPTIONS\0") {
        c.set_options(o);
    }
    if c.nns == 0 {
        c.ns[0] = [127, 0, 0, 1];
        c.nns = 1;
    }
    store_conf(st, &c);
}

/// Write a `Conf` into `_res`.
fn store_conf(st: &mut ResState, c: &Conf) {
    st.retrans = c.timeout;
    st.retry = c.attempts;
    st.options = c.options | RES_INIT;
    st.nscount = i32::try_from(c.nns).unwrap_or(0);
    for (slot, ip) in st.nsaddr_list.iter_mut().zip(c.ns.iter()) {
        *slot = crate::socket::SockaddrIn {
            sin_family: crate::socket::AF_INET as u16,
            sin_port: 53u16.to_be(),
            sin_addr: crate::socket::InAddr {
                s_addr: u32::from_ne_bytes(*ip),
            },
            sin_zero: [0; 8],
        };
    }
    st.bits = (st.bits & !0xf) | (c.ndots.clamp(0, RES_MAXNDOTS) as u32);
    st.defdname = [0; 256];
    st.dnsrch = [core::ptr::null_mut(); MAXDNSRCH + 1];
    let mut at = 0usize;
    for i in 0..c.nsearch {
        let (Some(dom), Some(&len)) = (c.search.get(i), c.search_len.get(i)) else {
            break;
        };
        let Some(dst) = st.defdname.get_mut(at..at + len + 1) else {
            break;
        };
        if let (Some(body), Some(src)) = (dst.get_mut(..len), dom.get(..len)) {
            body.copy_from_slice(src);
        }
        if let Some(slot) = st.dnsrch.get_mut(i) {
            *slot = st.defdname.as_mut_ptr().wrapping_add(at);
        }
        at += len + 1;
    }
}

/// The nameservers and timing a query uses, from `_res`.
struct Servers {
    addrs: [crate::socket::SockaddrIn; MAXNS],
    n: usize,
    timeout_ms: u64,
    attempts: u64,
    tcp_only: bool,
}

fn servers(st: &ResState) -> Servers {
    let n = usize::try_from(st.nscount).unwrap_or(0).min(MAXNS);
    let mut addrs = st.nsaddr_list;
    if st.options & RES_ROTATE != 0 && n > 1 {
        // glibc rotates one step per query; which server leads is all that
        // matters, since every one is asked.
        if let Some(a) = addrs.get_mut(..n) {
            a.rotate_left(1);
        }
    }
    Servers {
        addrs,
        n,
        timeout_ms: u64::try_from(st.retrans.clamp(1, RES_MAXRETRANS)).unwrap_or(5) * 1000,
        attempts: u64::try_from(st.retry.clamp(1, RES_MAXRETRY)).unwrap_or(2),
        tcp_only: st.options & RES_USEVC != 0,
    }
}

/// `res_init` -- (re)read `resolv.conf` and the environment into `_res`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_init() -> i32 {
    // SAFETY: as in `state`.
    load_state(unsafe { &mut *res_storage() });
    0
}

/// `__res_init` -- glibc alias for `res_init`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __res_init() -> i32 {
    res_init()
}

// ---------------------------------------------------------------------------
// Names: glibc's ns_name_* functions
// ---------------------------------------------------------------------------

/// glibc's `res_hnok`: whether `dn` is a host name -- printable ASCII, a
/// name `ns_name_pton` accepts, and every label only letters, digits, `-`
/// and `_`, the first not starting with `-`.  The DNS module asks about
/// nothing else.
pub(crate) fn res_hnok(dn: &[u8]) -> bool {
    if !dn.iter().all(|&c| c > b' ' && c <= b'~') {
        return false;
    }
    let mut wire = [0u8; 255];
    let Ok((n, _)) = name_pton(dn, &mut wire) else {
        return false;
    };
    let wire = wire.get(..n).unwrap_or(&[]);
    if wire.first().is_some_and(|&l| l > 0) && wire.get(1) == Some(&b'-') {
        return false;
    }
    let mut i = 0usize;
    while let Some(&len) = wire.get(i) {
        if len == 0 {
            break;
        }
        let label = wire.get(i + 1..i + 1 + usize::from(len)).unwrap_or(&[]);
        if !label
            .iter()
            .all(|&c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return false;
        }
        i += 1 + usize::from(len);
    }
    true
}

/// `ns_name_pton`: text to an uncompressed wire name in `dst`; the bytes
/// written, and whether the text was fully qualified.  `Err` is glibc's
/// `EMSGSIZE`.
fn name_pton(src: &[u8], dst: &mut [u8]) -> Result<(usize, bool), ()> {
    let mut label = 0usize; // index of the current label's length byte
    let mut bp = 1usize; // next byte to write
    if dst.is_empty() {
        return Err(());
    }
    let mut i = 0usize;
    let mut escaped = false;
    while let Some(&c0) = src.get(i) {
        if c0 == 0 {
            break;
        }
        i += 1;
        let mut c = c0;
        if escaped {
            if c.is_ascii_digit() {
                let d2 = *src.get(i).ok_or(())?;
                let d3 = *src.get(i + 1).ok_or(())?;
                if !d2.is_ascii_digit() || !d3.is_ascii_digit() {
                    return Err(());
                }
                i += 2;
                let n =
                    u32::from(c - b'0') * 100 + u32::from(d2 - b'0') * 10 + u32::from(d3 - b'0');
                c = u8::try_from(n).map_err(|_| ())?;
            }
            escaped = false;
        } else if c == b'\\' {
            escaped = true;
            continue;
        } else if c == b'.' {
            let len = bp - label - 1;
            if len > MAXLABEL {
                return Err(());
            }
            *dst.get_mut(label).ok_or(())? = len as u8;
            // Fully qualified?
            if matches!(src.get(i), None | Some(0)) {
                if len != 0 {
                    *dst.get_mut(bp).ok_or(())? = 0;
                    bp += 1;
                }
                if bp > MAXCDNAME {
                    return Err(());
                }
                return Ok((bp, true));
            }
            if len == 0 || src.get(i) == Some(&b'.') {
                return Err(());
            }
            label = bp;
            bp += 1;
            continue;
        }
        *dst.get_mut(bp).ok_or(())? = c;
        bp += 1;
    }
    if escaped {
        return Err(());
    }
    let len = bp - label - 1;
    if len > MAXLABEL {
        return Err(());
    }
    *dst.get_mut(label).ok_or(())? = len as u8;
    if len != 0 {
        *dst.get_mut(bp).ok_or(())? = 0;
        bp += 1;
    }
    if bp > MAXCDNAME {
        return Err(());
    }
    Ok((bp, false))
}

/// glibc's `special`: characters `ns_name_ntop` escapes with a backslash.
fn special(c: u8) -> bool {
    matches!(c, b'"' | b'.' | b';' | b'\\' | b'(' | b')' | b'@' | b'$')
}

/// `ns_name_ntop`: an uncompressed wire name to NUL-terminated text in
/// `dst`; the bytes written with the NUL.  The root is `"."`.
fn name_ntop(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
    let mut dn = 0usize;
    let mut cp = 0usize;
    let mut put = |dn: &mut usize, b: u8| -> Result<(), ()> {
        *dst.get_mut(*dn).ok_or(())? = b;
        *dn += 1;
        Ok(())
    };
    loop {
        let l = usize::from(*src.get(cp).ok_or(())?);
        cp += 1;
        if l == 0 {
            break;
        }
        if l >= 64 {
            return Err(());
        }
        if dn != 0 {
            put(&mut dn, b'.')?;
        }
        for _ in 0..l {
            let c = *src.get(cp).ok_or(())?;
            cp += 1;
            if special(c) {
                put(&mut dn, b'\\')?;
                put(&mut dn, c)?;
            } else if !(0x21..0x7f).contains(&c) {
                put(&mut dn, b'\\')?;
                put(&mut dn, b'0' + c / 100)?;
                put(&mut dn, b'0' + (c % 100) / 10)?;
                put(&mut dn, b'0' + c % 10)?;
            } else {
                put(&mut dn, c)?;
            }
        }
    }
    if dn == 0 {
        put(&mut dn, b'.')?;
    }
    put(&mut dn, 0)?;
    Ok(dn)
}

/// `ns_name_unpack`: the name at `msg[at..]`, compression pointers
/// followed, into `dst` uncompressed; the bytes the name occupies at `at`.
fn name_unpack(msg: &[u8], at: usize, dst: &mut [u8]) -> Result<usize, ()> {
    if at >= msg.len() {
        return Err(());
    }
    let mut srcp = at;
    let mut dstp = 0usize;
    let mut len: Option<usize> = None;
    let mut checked = 0usize;
    loop {
        let n = *msg.get(srcp).ok_or(())?;
        srcp += 1;
        if n == 0 {
            break;
        }
        match n & 0xc0 {
            0 => {
                let n = usize::from(n);
                // `n + 1 >=` covers the NUL written at the end.
                if n + 1 >= dst.len() - dstp || n > msg.len() - srcp {
                    return Err(());
                }
                checked += n + 1;
                *dst.get_mut(dstp).ok_or(())? = n as u8;
                dstp += 1;
                dst.get_mut(dstp..dstp + n)
                    .ok_or(())?
                    .copy_from_slice(msg.get(srcp..srcp + n).ok_or(())?);
                dstp += n;
                srcp += n;
            }
            0xc0 => {
                let lo = *msg.get(srcp).ok_or(())?;
                if len.is_none() {
                    len = Some(srcp + 1 - at);
                }
                let target = (usize::from(n & 0x3f) << 8) | usize::from(lo);
                if target >= msg.len() {
                    return Err(());
                }
                srcp = target;
                checked += 2;
                // Having looked at the whole message, there must be a loop.
                if checked >= msg.len() {
                    return Err(());
                }
            }
            _ => return Err(()),
        }
    }
    *dst.get_mut(dstp).ok_or(())? = 0;
    Ok(len.unwrap_or(srcp - at))
}

/// glibc's `dn_find`: the offset from `msg` of a name among the `dnptrs`
/// entries -- or a suffix of one -- equal to the wire name at `domain`,
/// compared without case.
///
/// # Safety
///
/// `msg` and every pointer in `dnptrs[..count]` point into one readable
/// message, and `domain` to a valid uncompressed wire name.
unsafe fn dn_find(
    domain: *const u8,
    msg: *const u8,
    dnptrs: *const *mut u8,
    count: usize,
) -> Option<usize> {
    for k in 0..count {
        // SAFETY: the caller's contract.
        let mut sp = unsafe { *dnptrs.add(k) }.cast_const();
        loop {
            // SAFETY: as above -- names in the message are well formed.
            let first = unsafe { *sp };
            let off = (sp as usize).wrapping_sub(msg as usize);
            if first == 0 || first & 0xc0 != 0 || off >= 0x4000 {
                break;
            }
            let mut dn = domain;
            let mut cp = sp;
            let found = 'cmp: loop {
                // SAFETY: as above.
                let n = unsafe { *cp };
                cp = cp.wrapping_add(1);
                if n == 0 {
                    break 'cmp false;
                }
                match n & 0xc0 {
                    0 => {
                        // SAFETY: as above.
                        if n != unsafe { *dn } {
                            break 'cmp false;
                        }
                        dn = dn.wrapping_add(1);
                        for _ in 0..n {
                            // SAFETY: as above.
                            let (a, b) = unsafe { (*dn, *cp) };
                            if !a.eq_ignore_ascii_case(&b) {
                                break 'cmp false;
                            }
                            dn = dn.wrapping_add(1);
                            cp = cp.wrapping_add(1);
                        }
                        // SAFETY: as above.
                        let (a, b) = unsafe { (*dn, *cp) };
                        if a == 0 && b == 0 {
                            break 'cmp true;
                        }
                        if a == 0 {
                            break 'cmp false;
                        }
                    }
                    0xc0 => {
                        // SAFETY: as above.
                        let lo = unsafe { *cp };
                        cp = msg.wrapping_add((usize::from(n & 0x3f) << 8) | usize::from(lo));
                    }
                    _ => break 'cmp false,
                }
            };
            if found {
                return Some(off);
            }
            // SAFETY: as above.
            sp = sp.wrapping_add(usize::from(unsafe { *sp }) + 1);
        }
    }
    None
}

/// `ns_name_pack`: the uncompressed wire name `src` into `dst`, compressed
/// against the names `dnptrs` lists; the bytes written.  With `lastdnptr`,
/// the new name is added to the list.  glibc's algorithm, over glibc's
/// pointer table.
///
/// # Safety
///
/// `dst` is writable for `dstsiz` bytes; `dnptrs` is NULL or glibc's table
/// (the message start, then names in it, then NULL) whose array ends at
/// `lastdnptr`.
unsafe fn name_pack(
    src: &[u8],
    dst: *mut u8,
    dstsiz: usize,
    dnptrs: *mut *mut u8,
    lastdnptr: *mut *mut u8,
) -> Result<usize, ()> {
    // The table: its message, and where its list ends.
    let mut msg: *const u8 = core::ptr::null();
    let mut list: *mut *mut u8 = core::ptr::null_mut();
    let mut count = 0usize;
    if !dnptrs.is_null() {
        // SAFETY: the caller's contract.
        unsafe {
            msg = (*dnptrs).cast_const();
            if !msg.is_null() {
                list = dnptrs.add(1);
                while !(*list.add(count)).is_null() {
                    count += 1;
                }
            }
        }
    }
    // The name is legal.
    let mut total = 0usize;
    let mut i = 0usize;
    loop {
        let n = usize::from(*src.get(i).ok_or(())?);
        if n >= 64 {
            return Err(());
        }
        total += n + 1;
        if total > MAXCDNAME {
            return Err(());
        }
        i += n + 1;
        if n == 0 {
            break;
        }
    }
    // The names this call may point at: those listed when it began, not the
    // one it adds (glibc fixes `lpp` before packing).
    let searchable = count;
    let mut out = 0usize;
    let mut first = true;
    let mut added: Option<usize> = None;
    let mut srcp = 0usize;
    let fail = |added: Option<usize>| {
        if let Some(k) = added {
            // SAFETY: `k` is where this call wrote its entry.
            unsafe { *list.add(k) = core::ptr::null_mut() };
        }
        Err(())
    };
    loop {
        let n = usize::from(*src.get(srcp).ok_or(())?);
        if n != 0 && !msg.is_null() {
            // SAFETY: the caller's contract; `src[srcp..]` is a legal name.
            if let Some(off) = unsafe { dn_find(src.as_ptr().add(srcp), msg, list, searchable) } {
                if dstsiz - out <= 1 {
                    return fail(added);
                }
                // SAFETY: two bytes of `dst` remain.
                unsafe {
                    *dst.add(out) = 0xc0 | (off >> 8) as u8;
                    *dst.add(out + 1) = (off & 0xff) as u8;
                }
                return Ok(out + 2);
            }
            // Not found: remember where this name starts.
            let pos = (dst as usize).wrapping_add(out).wrapping_sub(msg as usize);
            let room = !lastdnptr.is_null()
                && (list.wrapping_add(count) as usize)
                    < (lastdnptr as usize).wrapping_sub(size_of::<*mut u8>());
            if room && pos < 0x4000 && first {
                // SAFETY: `room` says the slot and the NULL after it are
                // inside the table.
                unsafe {
                    *list.add(count) = dst.add(out);
                    *list.add(count + 1) = core::ptr::null_mut();
                }
                added = Some(count);
                count += 1;
                first = false;
            }
        }
        if n + 1 > dstsiz - out {
            return fail(added);
        }
        // SAFETY: `n + 1` bytes of `dst` remain, and of `src` (checked above).
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr().add(srcp), dst.add(out), n + 1);
        }
        srcp += n + 1;
        out += n + 1;
        if n == 0 {
            break;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// dn_expand / dn_skipname / dn_comp
// ---------------------------------------------------------------------------

/// `dn_expand` -- the compressed name at `comp_dn`, in the message
/// `[msg, eomorig)`, as text in `exp_dn` (`length` bytes); the bytes the
/// name occupies at `comp_dn`, or -1 with `EMSGSIZE`.
///
/// glibc's `ns_name_uncompress`, then the root's `"."` made `""`: special
/// characters come back escaped (`\.`, `\DDD`), and compression pointers
/// may point forward (only a loop is refused).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dn_expand(
    msg: *const u8,
    eomorig: *const u8,
    comp_dn: *const u8,
    exp_dn: *mut u8,
    length: i32,
) -> i32 {
    let result = (|| {
        let msg_len = (eomorig as usize).checked_sub(msg as usize).ok_or(())?;
        if msg.is_null() || exp_dn.is_null() {
            return Err(());
        }
        let at = (comp_dn as usize).checked_sub(msg as usize).ok_or(())?;
        // SAFETY: the caller's contract: `[msg, eomorig)` is the message.
        let whole = unsafe { core::slice::from_raw_parts(msg, msg_len) };
        let mut tmp = [0u8; MAXCDNAME];
        let n = name_unpack(whole, at, &mut tmp)?;
        let cap = usize::try_from(length).map_err(|_| ())?;
        // SAFETY: the caller's contract: `exp_dn` holds `length` bytes.
        let out = unsafe { core::slice::from_raw_parts_mut(exp_dn, cap) };
        name_ntop(&tmp, out)?;
        if out.first() == Some(&b'.') {
            if let Some(b) = out.get_mut(0) {
                *b = 0;
            }
        }
        i32::try_from(n).map_err(|_| ())
    })();
    result.unwrap_or_else(|()| {
        errno::set_errno(errno::EMSGSIZE);
        -1
    })
}

/// `dn_skipname` -- the bytes the compressed name at `comp_dn` occupies
/// (a pointer ends it, as two bytes), or -1 with `EMSGSIZE`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dn_skipname(comp_dn: *const u8, eom: *const u8) -> i32 {
    let avail = (eom as usize).saturating_sub(comp_dn as usize);
    if comp_dn.is_null() {
        errno::set_errno(errno::EMSGSIZE);
        return -1;
    }
    // SAFETY: the caller's contract: `[comp_dn, eom)` is readable.
    let s = unsafe { core::slice::from_raw_parts(comp_dn, avail) };
    let mut cp = 0usize;
    while let Some(&n) = s.get(cp) {
        cp += 1;
        if n == 0 {
            return i32::try_from(cp).unwrap_or(-1);
        }
        match n & 0xc0 {
            0 => {
                if s.len() - cp < usize::from(n) {
                    break;
                }
                cp += usize::from(n);
            }
            0xc0 => {
                if cp == s.len() {
                    break;
                }
                return i32::try_from(cp + 1).unwrap_or(-1);
            }
            _ => break,
        }
    }
    errno::set_errno(errno::EMSGSIZE);
    -1
}

/// `dn_comp` -- the text name `exp_dn` into `comp_dn` (`length` bytes),
/// compressed against the names in `dnptrs`; the bytes written, or -1 with
/// `EMSGSIZE`.  glibc's `ns_name_compress`: until 2026-09-26 this wrote no
/// compression pointers, left `dnptrs` alone, and did not unescape.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dn_comp(
    exp_dn: *const u8,
    comp_dn: *mut u8,
    length: i32,
    dnptrs: *mut *mut u8,
    lastdnptr: *mut *mut u8,
) -> i32 {
    let result = (|| {
        if exp_dn.is_null() || comp_dn.is_null() {
            return Err(());
        }
        // SAFETY: the caller's contract: a NUL-terminated name.
        let text = unsafe { core::slice::from_raw_parts(exp_dn, crate::string::strlen(exp_dn)) };
        let mut tmp = [0u8; MAXCDNAME];
        name_pton(text, &mut tmp)?;
        let cap = usize::try_from(length).map_err(|_| ())?;
        // SAFETY: the caller's contract for `comp_dn`, `dnptrs`, `lastdnptr`.
        let n = unsafe { name_pack(&tmp, comp_dn, cap, dnptrs, lastdnptr) }?;
        i32::try_from(n).map_err(|_| ())
    })();
    result.unwrap_or_else(|()| {
        errno::set_errno(errno::EMSGSIZE);
        -1
    })
}

// ---------------------------------------------------------------------------
// ns_get16 / ns_get32 / ns_put16 / ns_put32
// ---------------------------------------------------------------------------

/// `ns_get16` — get a 16-bit value from network byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_get16(src: *const u8) -> u16 {
    if src.is_null() {
        return 0;
    }
    // SAFETY: caller guarantees at least 2 bytes.
    let b = unsafe { core::ptr::read_unaligned(src.cast::<[u8; 2]>()) };
    u16::from_be_bytes(b)
}

/// `ns_get32` — get a 32-bit value from network byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_get32(src: *const u8) -> u32 {
    if src.is_null() {
        return 0;
    }
    // SAFETY: caller guarantees at least 4 bytes.
    let b = unsafe { core::ptr::read_unaligned(src.cast::<[u8; 4]>()) };
    u32::from_be_bytes(b)
}

/// `ns_put16` — put a 16-bit value in network byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_put16(val: u16, dst: *mut u8) {
    if dst.is_null() {
        return;
    }
    // SAFETY: caller guarantees at least 2 bytes.
    unsafe { core::ptr::write_unaligned(dst.cast::<[u8; 2]>(), val.to_be_bytes()) };
}

/// `ns_put32` — put a 32-bit value in network byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_put32(val: u32, dst: *mut u8) {
    if dst.is_null() {
        return;
    }
    // SAFETY: caller guarantees at least 4 bytes.
    unsafe { core::ptr::write_unaligned(dst.cast::<[u8; 4]>(), val.to_be_bytes()) };
}

// ---------------------------------------------------------------------------
// res_mkquery
// ---------------------------------------------------------------------------

/// glibc's `__res_context_mkquery`, into `buf`: the query's length.
fn mkquery(
    st: &ResState,
    op: i32,
    dname: &[u8],
    class: i32,
    type_: i32,
    data: Option<&[u8]>,
    buf: &mut [u8],
) -> Result<usize, ()> {
    let class = u16::try_from(class).map_err(|_| ())?;
    let type_ = u16::try_from(type_).map_err(|_| ())?;
    if buf.len() < HFIXEDSZ {
        return Err(());
    }
    let header = buf.get_mut(..HFIXEDSZ).ok_or(())?;
    header.fill(0);
    let id = (crate::random::arc4random() & 0xffff) as u16;
    header[..2].copy_from_slice(&id.to_be_bytes());
    let op_bits = u8::try_from(op & 0xf).map_err(|_| ())?;
    header[2] = (op_bits << 3) | u8::from(st.options & RES_RECURSE != 0);
    header[3] = if st.options & RES_TRUSTAD != 0 {
        0x20
    } else {
        0
    };
    let extra = match op {
        QUERY => QFIXEDSZ,
        NS_NOTIFY_OP => QFIXEDSZ + if data.is_some() { RRFIXEDSZ } else { 0 },
        _ => return Err(()),
    };
    let room = buf.len().checked_sub(HFIXEDSZ + extra).ok_or(())?;
    let mut wire = [0u8; MAXCDNAME];
    name_pton(dname, &mut wire)?;
    let mut ptrs: [*mut u8; 20] = [core::ptr::null_mut(); 20];
    ptrs[0] = buf.as_mut_ptr();
    let last = ptrs.as_mut_ptr().wrapping_add(ptrs.len());
    // SAFETY: `buf[HFIXEDSZ..]` holds `room` bytes past the fixed parts;
    // the table is glibc's shape and inside `ptrs`.
    let n = unsafe {
        name_pack(
            &wire,
            buf.as_mut_ptr().add(HFIXEDSZ),
            room,
            ptrs.as_mut_ptr(),
            last,
        )
    }?;
    let mut at = HFIXEDSZ + n;
    let mut put16 = |at: &mut usize, v: u16| -> Result<(), ()> {
        buf.get_mut(*at..*at + 2)
            .ok_or(())?
            .copy_from_slice(&v.to_be_bytes());
        *at += 2;
        Ok(())
    };
    put16(&mut at, type_)?;
    put16(&mut at, class)?;
    // qdcount = 1
    buf.get_mut(4..6)
        .ok_or(())?
        .copy_from_slice(&1u16.to_be_bytes());
    if op == NS_NOTIFY_OP {
        if let Some(d) = data {
            // The completion domain, as an additional record.
            let mut w2 = [0u8; MAXCDNAME];
            name_pton(d, &mut w2)?;
            let room2 = buf.len().checked_sub(at + RRFIXEDSZ).ok_or(())?;
            // SAFETY: as above.
            let n2 = unsafe {
                name_pack(
                    &w2,
                    buf.as_mut_ptr().add(at),
                    room2,
                    ptrs.as_mut_ptr(),
                    last,
                )
            }?;
            at += n2;
            let rr = buf.get_mut(at..at + RRFIXEDSZ).ok_or(())?;
            rr.fill(0);
            rr[..2].copy_from_slice(&(T_NULL as u16).to_be_bytes());
            rr[2..4].copy_from_slice(&class.to_be_bytes());
            at += RRFIXEDSZ;
            buf.get_mut(10..12)
                .ok_or(())?
                .copy_from_slice(&1u16.to_be_bytes());
        }
    }
    Ok(at)
}

/// `res_mkquery` -- build a query message in `buf`: its length, or -1.
///
/// glibc 2.39's: `op` is `QUERY` or `NS_NOTIFY_OP` (`IQUERY` is no longer
/// built); `class` and `type` are 16-bit; the name follows `dn_comp`'s
/// rules; the id is random; the RD bit follows `RES_RECURSE`, the AD bit
/// `RES_TRUSTAD`.  `newrr` is unused, as in glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_mkquery(
    op: i32,
    dname: *const u8,
    class: i32,
    type_: i32,
    data: *const u8,
    _datalen: i32,
    _newrr: *const u8,
    buf: *mut u8,
    buflen: i32,
) -> i32 {
    let Ok(len) = usize::try_from(buflen) else {
        return -1;
    };
    if buf.is_null() || len < HFIXEDSZ || dname.is_null() {
        return -1;
    }
    // SAFETY: the caller's contract: NUL-terminated strings, and `buf`
    // holds `buflen` bytes.
    let (name, data, out) = unsafe {
        (
            core::slice::from_raw_parts(dname, crate::string::strlen(dname)),
            (!data.is_null())
                .then(|| core::slice::from_raw_parts(data, crate::string::strlen(data))),
            core::slice::from_raw_parts_mut(buf, len),
        )
    };
    match mkquery(state(), op, name, class, type_, data, out) {
        Ok(n) => i32::try_from(n).unwrap_or(-1),
        Err(()) => -1,
    }
}

// ---------------------------------------------------------------------------
// res_send: the transport
// ---------------------------------------------------------------------------

/// What a query's exchange with the nameservers needs from the network --
/// so the algorithm can be tested against a scripted one.
trait Transport {
    /// Send the query to server `i` over UDP.
    fn send_udp(&mut self, i: usize, q: &[u8]);
    /// Wait up to `ms` for a datagram: its length and the server it came
    /// from (`None` if not from one of ours), or `None` if nothing came.
    fn recv_udp(&mut self, buf: &mut [u8], ms: u64) -> Option<(usize, Option<usize>, bool)>;
    /// Ask server `i` over TCP, within `ms`: the answer's full length (which
    /// may exceed `buf`), or `None`.
    fn tcp(&mut self, i: usize, q: &[u8], buf: &mut [u8], ms: u64) -> Option<usize>;
    /// Milliseconds on a monotonic clock.
    fn now_ms(&mut self) -> u64;
}

/// How an answer's code is taken (glibc's `send_dg`): usable, or a server's
/// refusal to wait past.
fn usable_rcode(rcode: u8) -> bool {
    !matches!(rcode, SERVFAIL | NOTIMP | REFUSED)
}

/// Ask every server, again every `timeout / attempts`, until one gives a
/// usable answer or the time runs out: the answer's full length, or the
/// errno: `ETIMEDOUT` if some server answered unusably, else
/// `ECONNREFUSED` -- glibc's pair.  A truncated answer is asked again over
/// TCP.  musl's `__res_msend`, for one query.
fn exchange(
    t: &mut dyn Transport,
    sv: &Servers,
    q: &[u8],
    answer: &mut [u8],
) -> Result<usize, i32> {
    if sv.n == 0 {
        return Err(errno::ECONNREFUSED);
    }
    if sv.tcp_only {
        for i in 0..sv.n {
            if let Some(n) = t.tcp(i, q, answer, sv.timeout_ms) {
                return Ok(n);
            }
        }
        return Err(errno::ECONNREFUSED);
    }
    let retry = (sv.timeout_ms / sv.attempts).max(1);
    let t0 = t.now_ms();
    let mut sent_at: Option<u64> = None;
    let mut heard_something = false;
    let mut servfail_retries = 2u32;
    loop {
        let now = t.now_ms();
        if now.wrapping_sub(t0) >= sv.timeout_ms {
            break;
        }
        if sent_at.is_none_or(|s| now.wrapping_sub(s) >= retry) {
            for i in 0..sv.n {
                t.send_udp(i, q);
            }
            sent_at = Some(now);
            servfail_retries = 2;
        }
        let waited = now.wrapping_sub(sent_at.unwrap_or(now));
        let Some((n, from, truncated)) = t.recv_udp(answer, retry.saturating_sub(waited).max(1))
        else {
            continue;
        };
        // Not from a server we asked, too short to identify, or another
        // query's answer: ignore it.
        let Some(server) = from else { continue };
        if n < 4 || answer.get(..2) != q.get(..2) {
            continue;
        }
        heard_something = true;
        let rcode = answer.get(3).map_or(SERVFAIL, |b| b & 0xf);
        if !usable_rcode(rcode) {
            if rcode == SERVFAIL && servfail_retries > 0 {
                servfail_retries -= 1;
                t.send_udp(server, q);
            }
            continue;
        }
        let tc = answer.get(2).is_some_and(|b| b & 0x02 != 0);
        if tc || truncated {
            let left = sv
                .timeout_ms
                .saturating_sub(t.now_ms().wrapping_sub(t0))
                .max(1);
            if let Some(full) = t.tcp(server, q, answer, left) {
                return Ok(full);
            }
            continue;
        }
        return Ok(n);
    }
    Err(if heard_something {
        errno::ETIMEDOUT
    } else {
        errno::ECONNREFUSED
    })
}

/// The real network: one UDP socket, a TCP connection per fallback.
struct Net {
    udp: i32,
    servers: [crate::socket::SockaddrIn; MAXNS],
    n: usize,
}

impl Net {
    fn open(sv: &Servers) -> Result<Self, i32> {
        let fd = crate::socket::socket(
            crate::socket::AF_INET,
            crate::socket::SOCK_DGRAM | crate::socket::SOCK_NONBLOCK | crate::socket::SOCK_CLOEXEC,
            0,
        );
        if fd < 0 {
            return Err(errno::get_errno());
        }
        Ok(Self {
            udp: fd,
            servers: sv.addrs,
            n: sv.n,
        })
    }

    fn poll_one(fd: i32, events: i16, ms: u64) -> bool {
        let mut p = crate::poll::Pollfd {
            fd,
            events,
            revents: 0,
        };
        let ms = i32::try_from(ms).unwrap_or(i32::MAX);
        // SAFETY: one valid `Pollfd`.
        unsafe { crate::poll::poll(&raw mut p, 1, ms) > 0 && p.revents & events != 0 }
    }

    fn sockaddr(&self, i: usize) -> *const crate::socket::Sockaddr {
        self.servers
            .get(i)
            .map_or(core::ptr::null(), |s| core::ptr::from_ref(s).cast())
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        // Nothing was written that a close could lose.
        let _ = crate::file::close(self.udp);
    }
}

const SIN_LEN: crate::socket::SocklenT =
    size_of::<crate::socket::SockaddrIn>() as crate::socket::SocklenT;

impl Transport for Net {
    fn send_udp(&mut self, i: usize, q: &[u8]) {
        // SAFETY: `q` and the address are valid; a lost datagram is one
        // the retry sends again.
        let _ = unsafe {
            crate::socket::sendto(
                self.udp,
                q.as_ptr(),
                q.len(),
                crate::socket::MSG_NOSIGNAL,
                self.sockaddr(i),
                SIN_LEN,
            )
        };
    }

    fn recv_udp(&mut self, buf: &mut [u8], ms: u64) -> Option<(usize, Option<usize>, bool)> {
        if !Self::poll_one(self.udp, crate::poll::POLLIN, ms) {
            return None;
        }
        let mut from = SIN_ZERO;
        let mut len = SIN_LEN;
        // SAFETY: `buf` and `from` are valid for their lengths.
        let r = unsafe {
            crate::socket::recvfrom(
                self.udp,
                buf.as_mut_ptr(),
                buf.len(),
                0,
                (&raw mut from).cast(),
                &raw mut len,
            )
        };
        let n = usize::try_from(r).ok()?;
        let server =
            self.servers.get(..self.n)?.iter().position(|s| {
                s.sin_addr.s_addr == from.sin_addr.s_addr && s.sin_port == from.sin_port
            });
        // A server truncating an answer sets TC; a datagram cut by our
        // buffer (which is at least 512 bytes, the size a server may send a
        // plain query) cannot happen, and reads as TC-less and whole.
        Some((n, server, false))
    }

    fn tcp(&mut self, i: usize, q: &[u8], buf: &mut [u8], ms: u64) -> Option<usize> {
        let fd = crate::socket::socket(
            crate::socket::AF_INET,
            crate::socket::SOCK_STREAM | crate::socket::SOCK_CLOEXEC,
            0,
        );
        if fd < 0 {
            return None;
        }
        let result = (|| {
            // SAFETY: a valid address.
            if unsafe { crate::socket::connect(fd, self.sockaddr(i), SIN_LEN) } != 0 {
                return None;
            }
            let qlen = u16::try_from(q.len()).ok()?.to_be_bytes();
            for part in [&qlen[..], q] {
                let mut sent = 0usize;
                while let Some(rest) = part.get(sent..).filter(|r| !r.is_empty()) {
                    // SAFETY: `rest` is valid.
                    let r = unsafe {
                        crate::socket::send(
                            fd,
                            rest.as_ptr(),
                            rest.len(),
                            crate::socket::MSG_NOSIGNAL,
                        )
                    };
                    sent += usize::try_from(r).ok().filter(|&r| r > 0)?;
                }
            }
            let read_exact = |dst: &mut [u8]| -> Option<()> {
                let mut got = 0usize;
                while let Some(rest) = dst.get_mut(got..).filter(|r| !r.is_empty()) {
                    if !Self::poll_one(fd, crate::poll::POLLIN, ms) {
                        return None;
                    }
                    // SAFETY: `rest` is valid.
                    let r = unsafe { crate::socket::recv(fd, rest.as_mut_ptr(), rest.len(), 0) };
                    got += usize::try_from(r).ok().filter(|&r| r > 0)?;
                }
                Some(())
            };
            let mut lenb = [0u8; 2];
            read_exact(&mut lenb)?;
            let alen = usize::from(u16::from_be_bytes(lenb));
            if alen < HFIXEDSZ {
                return None;
            }
            // Keep what fits; report the whole length, as glibc does.
            let keep = alen.min(buf.len());
            read_exact(buf.get_mut(..keep)?)?;
            Some(alen)
        })();
        // Nothing unsent remains that a close could lose.
        let _ = crate::file::close(fd);
        result
    }

    fn now_ms(&mut self) -> u64 {
        let t = crate::lowlevellock::now_on(crate::time::CLOCK_MONOTONIC);
        u64::try_from(t.tv_sec).unwrap_or(0) * 1000
            + u64::try_from(t.tv_nsec / 1_000_000).unwrap_or(0)
    }
}

/// Send a query and receive its answer into `answer`: the answer's full
/// length (which may exceed `answer`), or the errno.
fn send_query(st: &ResState, q: &[u8], answer: &mut [u8]) -> Result<usize, i32> {
    let sv = servers(st);
    if answer.len() < PACKETSZ {
        // Receive into a whole datagram's room, keep what fits (musl).
        let mut tmp = [0u8; PACKETSZ];
        let n = send_query(st, q, &mut tmp)?;
        let keep = n.min(answer.len()).min(tmp.len());
        if let (Some(dst), Some(src)) = (answer.get_mut(..keep), tmp.get(..keep)) {
            dst.copy_from_slice(src);
        }
        return Ok(n);
    }
    let mut net = Net::open(&sv)?;
    exchange(&mut net, &sv, q, answer)
}

/// `res_send` -- send the query `msg` and receive its answer: the answer's
/// length -- all of it, even past `anslen`, so a caller can tell a
/// truncated copy -- or -1 with `errno`: `ESRCH` with no nameserver,
/// `EINVAL` for an answer buffer shorter than a header (glibc's two
/// checks), `ECONNREFUSED` when no server answered at all, `ETIMEDOUT` when
/// none answered usably in time.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_send(msg: *const u8, msglen: i32, answer: *mut u8, anslen: i32) -> i32 {
    let st = state();
    if st.nscount <= 0 {
        errno::set_errno(errno::ESRCH);
        return -1;
    }
    let (Ok(qlen), Ok(alen)) = (usize::try_from(msglen), usize::try_from(anslen)) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if alen < HFIXEDSZ {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if msg.is_null() || answer.is_null() {
        // glibc would fault reading or writing them.
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: the caller's contract.
    let (q, a) = unsafe {
        (
            core::slice::from_raw_parts(msg, qlen),
            core::slice::from_raw_parts_mut(answer, alen),
        )
    };
    match send_query(st, q, a) {
        Ok(n) => i32::try_from(n).unwrap_or(i32::MAX),
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// res_query / res_querydomain / res_search
// ---------------------------------------------------------------------------

/// How glibc's `__res_context_query` judges an answer: `Ok` if it answers,
/// else the `h_errno`.
fn judge(answer: &[u8]) -> Result<(), i32> {
    let rcode = answer.get(3).map_or(SERVFAIL, |b| b & 0xf);
    let ancount = answer
        .get(6..8)
        .map_or(0, |b| u16::from_be_bytes([b[0], b[1]]));
    if rcode == NOERROR && ancount != 0 {
        return Ok(());
    }
    Err(match rcode {
        NXDOMAIN => crate::socket::HOST_NOT_FOUND,
        SERVFAIL => crate::socket::TRY_AGAIN,
        NOERROR => crate::socket::NO_DATA,
        _ => crate::socket::NO_RECOVERY, // FORMERR, NOTIMP, REFUSED and the rest
    })
}

/// A query for `name`: the answer's length, or `Err` with `h_errno` (and
/// `errno` for a send failure) set.
fn query(
    st: &ResState,
    name: &[u8],
    class: i32,
    type_: i32,
    answer: &mut [u8],
) -> Result<usize, ()> {
    let mut q = [0u8; HFIXEDSZ + QFIXEDSZ + MAXCDNAME + 1];
    let Ok(ql) = mkquery(st, QUERY, name, class, type_, None, &mut q) else {
        crate::socket::set_h_errno(crate::socket::NO_RECOVERY);
        return Err(());
    };
    let q = q.get(..ql).unwrap_or(&[]);
    // A buffer too short for the header is answered through a whole one,
    // and gets what fits.
    let mut local = [0u8; PACKETSZ];
    let small = answer.len() < HFIXEDSZ;
    let (sent, verdict) = {
        let buf: &mut [u8] = if small { &mut local } else { &mut *answer };
        let sent = send_query(st, q, buf);
        let verdict = judge(buf);
        (sent, verdict)
    };
    if small {
        let keep = answer.len();
        if let (Some(dst), Some(src)) = (answer.get_mut(..keep), local.get(..keep)) {
            dst.copy_from_slice(src);
        }
    }
    match sent {
        Err(e) => {
            errno::set_errno(e);
            crate::socket::set_h_errno(crate::socket::TRY_AGAIN);
            Err(())
        }
        Ok(n) => match verdict {
            Ok(()) => Ok(n),
            Err(h) => {
                crate::socket::set_h_errno(h);
                Err(())
            }
        },
    }
}

/// The caller's name and answer buffer, or -1 (the `EFAULT` glibc would
/// fault into).
fn args<'a>(name: *const u8, answer: *mut u8, anslen: i32) -> Option<(&'a [u8], &'a mut [u8])> {
    let len = usize::try_from(anslen).unwrap_or(0);
    if name.is_null() || (answer.is_null() && len > 0) {
        errno::set_errno(errno::EFAULT);
        crate::socket::set_h_errno(NETDB_INTERNAL);
        return None;
    }
    // SAFETY: the caller's contract.
    unsafe {
        Some((
            core::slice::from_raw_parts(name, crate::string::strlen(name)),
            if answer.is_null() {
                &mut []
            } else {
                core::slice::from_raw_parts_mut(answer, len)
            },
        ))
    }
}

/// `res_query` -- query for `name` as it stands: the answer's length, or -1
/// with `h_errno`: `HOST_NOT_FOUND` (no such name), `NO_DATA` (no record of
/// that type), `TRY_AGAIN` (no server answered, or `SERVFAIL`),
/// `NO_RECOVERY` (a refusal, or a name that cannot be sent).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_query(
    dname: *const u8,
    class: i32,
    type_: i32,
    answer: *mut u8,
    anslen: i32,
) -> i32 {
    let Some((name, buf)) = args(dname, answer, anslen) else {
        return -1;
    };
    query(state(), name, class, type_, buf).map_or(-1, |n| i32::try_from(n).unwrap_or(i32::MAX))
}

/// `__res_query` — glibc alias for `res_query`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __res_query(
    dname: *const u8,
    class: i32,
    type_: i32,
    answer: *mut u8,
    anslen: i32,
) -> i32 {
    res_query(dname, class, type_, answer, anslen)
}

/// `name.domain` (or `name` without a trailing dot, for no domain), glibc's
/// `__res_context_querydomain`: `None` past `MAXDNAME`.
fn join(name: &[u8], domain: Option<&[u8]>, out: &mut [u8; MAXDNAME]) -> Option<usize> {
    match domain {
        None => {
            let n = name.strip_suffix(b".").unwrap_or(name);
            if name.len() >= MAXDNAME {
                return None;
            }
            out.get_mut(..n.len())?.copy_from_slice(n);
            Some(n.len())
        }
        Some(d) => {
            let total = name.len() + 1 + d.len();
            if total >= MAXDNAME {
                return None;
            }
            out.get_mut(..name.len())?.copy_from_slice(name);
            *out.get_mut(name.len())? = b'.';
            out.get_mut(name.len() + 1..total)?.copy_from_slice(d);
            Some(total)
        }
    }
}

fn querydomain(
    st: &ResState,
    name: &[u8],
    domain: Option<&[u8]>,
    class: i32,
    type_: i32,
    answer: &mut [u8],
) -> Result<usize, ()> {
    let mut full = [0u8; MAXDNAME];
    let Some(n) = join(name, domain, &mut full) else {
        crate::socket::set_h_errno(crate::socket::NO_RECOVERY);
        return Err(());
    };
    query(st, full.get(..n).unwrap_or(&[]), class, type_, answer)
}

/// `res_querydomain` -- query for `name.domain`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_querydomain(
    name: *const u8,
    domain: *const u8,
    class: i32,
    type_: i32,
    answer: *mut u8,
    anslen: i32,
) -> i32 {
    let Some((name, buf)) = args(name, answer, anslen) else {
        return -1;
    };
    // SAFETY: the caller's contract: NULL or a NUL-terminated string.
    let domain = (!domain.is_null())
        .then(|| unsafe { core::slice::from_raw_parts(domain, crate::string::strlen(domain)) });
    querydomain(state(), name, domain, class, type_, buf)
        .map_or(-1, |n| i32::try_from(n).unwrap_or(i32::MAX))
}

/// What one query of a search came to: the answer's length, or its
/// `h_errno`, whether the send failed with `ECONNREFUSED`, and whether the
/// answer was a `SERVFAIL`.
type Attempt = Result<usize, (i32, bool, bool)>;

/// glibc's `__res_context_search`: `domain` is `None` for the name as it
/// stands.  The first answer, or `Err` with the `h_errno` to report.
fn search_with(
    name: &[u8],
    ndots: usize,
    options: u64,
    domains: &[&[u8]],
    mut attempt: impl FnMut(Option<&[u8]>) -> Attempt,
) -> Result<usize, i32> {
    // Counted once per search, over a name of a few dozen bytes; the
    // `bytecount` crate's SIMD is not worth a dependency of the C library.
    #[allow(clippy::naive_bytecount)]
    let dots = name.iter().filter(|&&c| c == b'.').count();
    let trailing_dot = name.last() == Some(&b'.');
    let mut last_h = crate::socket::HOST_NOT_FOUND; // if nothing is queried
    let mut saved: Option<i32> = None;
    let mut tried_as_is = false;
    if dots >= ndots || trailing_dot {
        match attempt(None) {
            Ok(n) => return Ok(n),
            Err((h, _, _)) => {
                if trailing_dot {
                    return Err(h);
                }
                saved = Some(h);
                last_h = h;
                tried_as_is = true;
            }
        }
    }
    let mut searched = false;
    let mut root_on_list = false;
    let mut got_nodata = false;
    let mut got_servfail = false;
    if (dots == 0 && options & RES_DEFNAMES != 0)
        || (dots != 0 && !trailing_dot && options & RES_DNSRCH != 0)
    {
        for d in domains {
            searched = true;
            // "name." -- the root -- for a domain written "." or "".
            let d = d.strip_prefix(b".").unwrap_or(d);
            if d.is_empty() {
                root_on_list = true;
            }
            match attempt(Some(d)) {
                Ok(n) => return Ok(n),
                Err((h, refused, servfail)) => {
                    last_h = h;
                    if refused {
                        return Err(crate::socket::TRY_AGAIN);
                    }
                    let keep_going = match h {
                        crate::socket::NO_DATA => {
                            got_nodata = true;
                            true
                        }
                        crate::socket::HOST_NOT_FOUND => true,
                        crate::socket::TRY_AGAIN if servfail => {
                            got_servfail = true;
                            true
                        }
                        _ => false,
                    };
                    // Without RES_DNSRCH, one domain (RES_DEFNAMES's).
                    if !keep_going || options & RES_DNSRCH == 0 {
                        break;
                    }
                }
            }
        }
    }
    if (dots != 0 || !searched || options & RES_NOTLDQUERY == 0) && !(tried_as_is || root_on_list) {
        match attempt(None) {
            Ok(n) => return Ok(n),
            Err((h, _, _)) => last_h = h,
        }
    }
    Err(if let Some(h) = saved {
        h
    } else if got_nodata {
        crate::socket::NO_DATA
    } else if got_servfail {
        crate::socket::TRY_AGAIN
    } else {
        last_h
    })
}

fn search(
    st: &ResState,
    name: &[u8],
    class: i32,
    type_: i32,
    answer: &mut [u8],
) -> Result<usize, ()> {
    let mut domains: [&[u8]; MAXDNSRCH] = [&[]; MAXDNSRCH];
    let mut nd = 0usize;
    for p in st.dnsrch.iter().take(MAXDNSRCH) {
        if p.is_null() {
            break;
        }
        // SAFETY: `dnsrch` points into `defdname`, NUL-terminated.
        let d = unsafe {
            core::slice::from_raw_parts(p.cast_const(), crate::string::strlen(p.cast_const()))
        };
        if let Some(slot) = domains.get_mut(nd) {
            *slot = d;
            nd += 1;
        }
    }
    let result = search_with(
        name,
        st.ndots(),
        st.options,
        domains.get(..nd).unwrap_or(&[]),
        |domain| {
            errno::set_errno(0);
            querydomain(st, name, domain, class, type_, answer).map_err(|()| {
                let servfail = answer.get(3).is_some_and(|b| b & 0xf == SERVFAIL);
                (
                    crate::socket::get_h_errno(),
                    errno::get_errno() == errno::ECONNREFUSED,
                    servfail,
                )
            })
        },
    );
    result.map_err(crate::socket::set_h_errno)
}

/// `res_search` -- query for `name` through the search list, glibc's way:
/// as it stands first if it has `ndots` dots or ends in a dot, then with
/// each search domain (`RES_DNSRCH`; just the first with only
/// `RES_DEFNAMES`), then as it stands if not yet tried (unless
/// `RES_NOTLDQUERY` and it has no dots).  The first answer wins; otherwise
/// `h_errno` is the as-is query's, else `NO_DATA` if any domain had the
/// name, else `TRY_AGAIN` for a server failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_search(
    dname: *const u8,
    class: i32,
    type_: i32,
    answer: *mut u8,
    anslen: i32,
) -> i32 {
    let Some((name, buf)) = args(dname, answer, anslen) else {
        return -1;
    };
    search(state(), name, class, type_, buf).map_or(-1, |n| i32::try_from(n).unwrap_or(i32::MAX))
}

/// `__res_search` — glibc alias for `res_search`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __res_search(
    dname: *const u8,
    class: i32,
    type_: i32,
    answer: *mut u8,
    anslen: i32,
) -> i32 {
    res_search(dname, class, type_, answer, anslen)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn pton(s: &[u8]) -> Result<(Vec<u8>, bool), ()> {
        let mut out = [0u8; MAXCDNAME];
        let (n, q) = name_pton(s, &mut out)?;
        Ok((out[..n].to_vec(), q))
    }

    fn ntop(wire: &[u8]) -> Result<Vec<u8>, ()> {
        let mut out = [0u8; 1100];
        let n = name_ntop(wire, &mut out)?;
        Ok(out[..n - 1].to_vec())
    }

    const WWW: &[u8] = b"\x03www\x07example\x03com\x00";

    // -- names --

    #[test]
    fn test_pton() {
        assert_eq!(pton(b"www.example.com"), Ok((WWW.to_vec(), false)));
        assert_eq!(pton(b"www.example.com."), Ok((WWW.to_vec(), true)));
        assert_eq!(pton(b""), Ok((vec![0], false)));
        assert_eq!(pton(b"."), Ok((vec![0], true)));
        assert_eq!(pton(b"a\\.b.c"), Ok((b"\x03a.b\x01c\x00".to_vec(), false)));
        assert_eq!(pton(b"\\065bc"), Ok((b"\x03Abc\x00".to_vec(), false)));
        for bad in [&b"a..b"[..], b".a", b"a\\", b"\\25", b"\\256", b"a.."] {
            assert_eq!(pton(bad), Err(()), "{bad:?}");
        }
        let long_label = [b'x'; 64];
        assert_eq!(pton(&long_label), Err(()));
        assert!(pton(&[b'x'; 63]).is_ok());
        // 255 bytes of wire name at most.
        let long: Vec<u8> = core::iter::repeat_n(&b"abcdefghi."[..], 26)
            .flatten()
            .copied()
            .collect();
        assert_eq!(pton(&long), Err(()));
    }

    #[test]
    fn test_ntop() {
        assert_eq!(ntop(WWW), Ok(b"www.example.com".to_vec()));
        assert_eq!(ntop(b"\x00"), Ok(b".".to_vec()));
        assert_eq!(ntop(b"\x03a.b\x01c\x00"), Ok(b"a\\.b.c".to_vec()));
        assert_eq!(ntop(b"\x02\x01@\x00"), Ok(b"\\001\\@".to_vec()));
        assert_eq!(ntop(b"\x40"), Err(()), "a compression pointer");
        let mut tiny = [0u8; 4];
        assert_eq!(name_ntop(WWW, &mut tiny), Err(()));
    }

    #[test]
    fn test_pton_ntop_roundtrip() {
        for name in [&b"a\\.b.c"[..], b"\\001\\@x", b"example.org", b"\\\\.\\\""] {
            let (wire, _) = pton(name).expect("pton");
            assert_eq!(ntop(&wire).expect("ntop"), name);
        }
    }

    #[test]
    fn test_unpack_follows_pointers() {
        // "example.com" at 12, then "www" + a pointer to 12, at 25.
        let mut msg = vec![0u8; 12];
        msg.extend_from_slice(b"\x07example\x03com\x00");
        msg.extend_from_slice(b"\x03www\xc0\x0c");
        let mut out = [0u8; MAXCDNAME];
        assert_eq!(name_unpack(&msg, 25, &mut out), Ok(6));
        assert_eq!(&out[..WWW.len()], WWW);
        // A pointer that points forward is followed too.
        let mut fwd = vec![0u8; 12];
        fwd.extend_from_slice(b"\x03www\xc0\x12");
        fwd.extend_from_slice(b"\x07example\x03com\x00");
        assert_eq!(name_unpack(&fwd, 12, &mut out), Ok(6));
        assert_eq!(&out[..WWW.len()], WWW);
        // A loop, a pointer out of the message, a reserved label type, a
        // start past the end.
        assert_eq!(name_unpack(b"\xc0\x00", 0, &mut out), Err(()));
        assert_eq!(name_unpack(b"\xc0\x40", 0, &mut out), Err(()));
        assert_eq!(name_unpack(b"\x80", 0, &mut out), Err(()));
        assert_eq!(name_unpack(b"\x00", 1, &mut out), Err(()));
    }

    #[test]
    fn test_dn_expand_and_skipname() {
        let mut msg = vec![0u8; 12];
        msg.extend_from_slice(b"\x07example\x03com\x00");
        msg.extend_from_slice(b"\x03www\xc0\x0c");
        msg.push(0);
        let base = msg.as_ptr();
        let end = base.wrapping_add(msg.len());
        let mut out = [0u8; 64];
        let n = dn_expand(base, end, base.wrapping_add(25), out.as_mut_ptr(), 64);
        assert_eq!(n, 6);
        assert_eq!(&out[..16], b"www.example.com\0");
        // The root reads as "".
        let n = dn_expand(base, end, base.wrapping_add(31), out.as_mut_ptr(), 64);
        assert_eq!((n, out[0]), (1, 0));
        errno::set_errno(0);
        assert_eq!(
            dn_expand(base, end, base.wrapping_add(25), out.as_mut_ptr(), 5),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EMSGSIZE);
        assert_eq!(dn_skipname(base.wrapping_add(12), end), 13);
        assert_eq!(dn_skipname(base.wrapping_add(25), end), 6);
        errno::set_errno(0);
        assert_eq!(
            dn_skipname(base.wrapping_add(12), base.wrapping_add(15)),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EMSGSIZE);
    }

    #[test]
    fn test_dn_comp_compresses_against_the_table() {
        let mut msg = [0u8; 128];
        let base = msg.as_mut_ptr();
        let mut ptrs: [*mut u8; 8] = [core::ptr::null_mut(); 8];
        ptrs[0] = base;
        let last = ptrs.as_mut_ptr().wrapping_add(ptrs.len());
        // The first name has nothing to point at, and is remembered.
        let n1 = dn_comp(
            b"www.example.com\0".as_ptr(),
            base.wrapping_add(12),
            100,
            ptrs.as_mut_ptr(),
            last,
        );
        assert_eq!(n1, 17);
        assert_eq!(&msg[12..29], WWW);
        assert_eq!(ptrs[1], msg.as_mut_ptr().wrapping_add(12));
        // The second shares "example.com" -- matched without case.
        let at = 12 + 17;
        let n2 = dn_comp(
            b"MAIL.Example.COM\0".as_ptr(),
            msg.as_mut_ptr().wrapping_add(at),
            100,
            ptrs.as_mut_ptr(),
            last,
        );
        assert_eq!(n2, 7);
        assert_eq!(&msg[at..at + 7], b"\x04MAIL\xc0\x10");
        // A whole-name match is one pointer.
        let at2 = at + 7;
        let n3 = dn_comp(
            b"www.example.com\0".as_ptr(),
            msg.as_mut_ptr().wrapping_add(at2),
            100,
            ptrs.as_mut_ptr(),
            last,
        );
        assert_eq!(n3, 2);
        assert_eq!(&msg[at2..at2 + 2], b"\xc0\x0c");
    }

    #[test]
    fn test_dn_comp_without_a_table_and_its_errors() {
        let mut out = [0u8; 32];
        let none = core::ptr::null_mut();
        assert_eq!(
            dn_comp(b"a.b\0".as_ptr(), out.as_mut_ptr(), 32, none, none),
            5
        );
        assert_eq!(&out[..5], b"\x01a\x01b\x00");
        assert_eq!(dn_comp(b"\0".as_ptr(), out.as_mut_ptr(), 32, none, none), 1);
        errno::set_errno(0);
        assert_eq!(
            dn_comp(b"a.b\0".as_ptr(), out.as_mut_ptr(), 4, none, none),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EMSGSIZE);
        assert_eq!(
            dn_comp(b"a..b\0".as_ptr(), out.as_mut_ptr(), 32, none, none),
            -1
        );
    }

    // -- res_mkquery --

    fn fresh_state() -> ResState {
        let mut st = ResState::ZERO;
        let mut c = Conf::DEFAULT;
        c.ns[0] = [192, 0, 2, 53];
        c.nns = 1;
        store_conf(&mut st, &c);
        st
    }

    #[test]
    fn test_mkquery() {
        let st = fresh_state();
        let mut buf = [0u8; 64];
        let n = mkquery(&st, QUERY, b"www.example.com", C_IN, T_MX, None, &mut buf).expect("query");
        assert_eq!(n, HFIXEDSZ + WWW.len() + 4);
        assert_eq!(buf[2], 0x01, "QUERY with RD");
        assert_eq!(buf[3], 0, "no AD without trust-ad");
        assert_eq!(&buf[4..12], &[0, 1, 0, 0, 0, 0, 0, 0], "one question");
        assert_eq!(&buf[12..12 + WWW.len()], WWW);
        assert_eq!(&buf[n - 4..n], &[0, 15, 0, 1]);
        // 16-bit types, which musl refuses above 255.
        assert!(mkquery(&st, QUERY, b"x", C_IN, 257, None, &mut buf).is_ok());
        assert_eq!(
            mkquery(&st, QUERY, b"x", C_IN, 65536, None, &mut buf),
            Err(())
        );
        assert_eq!(mkquery(&st, QUERY, b"x", -1, T_A, None, &mut buf), Err(()));
        assert_eq!(
            mkquery(&st, IQUERY, b"x", C_IN, T_A, None, &mut buf),
            Err(())
        );
        assert_eq!(
            mkquery(
                &st,
                QUERY,
                b"www.example.com",
                C_IN,
                T_A,
                None,
                &mut buf[..20]
            ),
            Err(())
        );
        let mut norec = fresh_state();
        norec.options &= !RES_RECURSE;
        norec.options |= RES_TRUSTAD;
        mkquery(&norec, QUERY, b"x", C_IN, T_A, None, &mut buf).expect("query");
        assert_eq!((buf[2], buf[3]), (0, 0x20));
    }

    #[test]
    fn test_mkquery_notify_with_data() {
        let st = fresh_state();
        let mut buf = [0u8; 128];
        let n = mkquery(
            &st,
            NS_NOTIFY_OP,
            b"example.com",
            C_IN,
            T_SOA,
            Some(b"a.example.com"),
            &mut buf,
        )
        .expect("notify");
        assert_eq!(buf[2], (4 << 3) | 1);
        assert_eq!(&buf[10..12], &[0, 1], "one additional record");
        // The completion domain points into the question's name.
        let q_end = 12 + 13 + 4;
        assert_eq!(&buf[q_end..q_end + 4], b"\x01a\xc0\x0c");
        assert_eq!(n, q_end + 4 + RRFIXEDSZ);
    }

    #[test]
    fn test_res_mkquery_entry() {
        let mut buf = [0u8; 64];
        let none = core::ptr::null();
        let name = b"a.b\0".as_ptr();
        assert_eq!(
            res_mkquery(QUERY, name, C_IN, T_A, none, 0, none, buf.as_mut_ptr(), 64),
            12 + 5 + 4
        );
        assert_eq!(
            res_mkquery(QUERY, name, C_IN, T_A, none, 0, none, buf.as_mut_ptr(), 11),
            -1
        );
        assert_eq!(
            res_mkquery(QUERY, none, C_IN, T_A, none, 0, none, buf.as_mut_ptr(), 64),
            -1
        );
    }

    // -- resolv.conf --

    #[test]
    fn test_parse_conf() {
        let text = b"# comment\r\n; another\nnameserver 10.0.0.1\nnameserver ::1\n\
nameserver 10.0.0.2 # tail\nnameserver 10.0.0.3\nnameserver 10.0.0.4\n\
search a.example b.example\noptions ndots:3 timeout:99 attempts:0 rotate use-vc trust-ad\n";
        let c = parse_conf(text, b"host.example");
        assert_eq!(c.nns, 3, "IPv6 skipped, and four is one too many");
        assert_eq!(c.ns[..3], [[10, 0, 0, 1], [10, 0, 0, 2], [10, 0, 0, 3]]);
        assert_eq!(c.nsearch, 2);
        assert_eq!(&c.search[0][..c.search_len[0]], b"a.example");
        assert_eq!(&c.search[1][..c.search_len[1]], b"b.example");
        assert_eq!((c.ndots, c.timeout, c.attempts), (3, RES_MAXRETRANS, 1));
        let flags = RES_ROTATE | RES_USEVC | RES_TRUSTAD;
        assert_eq!(c.options & flags, flags);
        // The last of `domain` and `search` wins; with neither, the host's
        // domain is searched.
        let c = parse_conf(b"search one\ndomain two\n", b"");
        assert_eq!((c.nsearch, &c.search[0][..3]), (1, &b"two"[..]));
        let c = parse_conf(b"nameserver 1.2.3.4\n", b"corp.example");
        assert_eq!(&c.search[0][..c.search_len[0]], b"corp.example");
        let c = parse_conf(b"options ndots:99\n", b"");
        assert_eq!(c.ndots, RES_MAXNDOTS);
    }

    #[test]
    fn test_parse_ipv4() {
        assert_eq!(parse_ipv4(b"127.0.0.1"), Some([127, 0, 0, 1]));
        for bad in [
            &b"256.0.0.1"[..],
            b"1.2.3",
            b"1.2.3.4.5",
            b"::1",
            b"1.2.3.a",
            b"",
        ] {
            assert_eq!(parse_ipv4(bad), None);
        }
    }

    #[test]
    fn test_store_conf_fills_res() {
        let mut st = ResState::ZERO;
        let mut c = parse_conf(
            b"nameserver 10.1.2.3\nsearch x.example y.example\noptions ndots:2\n",
            b"",
        );
        c.timeout = 3;
        store_conf(&mut st, &c);
        assert_eq!((st.nscount, st.retrans, st.retry, st.ndots()), (1, 3, 2, 2));
        assert_eq!(st.options, RES_DEFAULT | RES_INIT);
        assert_eq!(st.nsaddr_list[0].sin_port, 53u16.to_be());
        assert_eq!(
            st.nsaddr_list[0].sin_addr.s_addr.to_ne_bytes(),
            [10, 1, 2, 3]
        );
        // SAFETY: `dnsrch[0]` points at a 9-byte domain in `defdname`.
        let d0 = unsafe { core::slice::from_raw_parts(st.dnsrch[0], 9) };
        assert_eq!(d0, b"x.example");
        assert!(st.dnsrch[2].is_null());
    }

    // -- the exchange --

    /// A scripted network: each UDP send is recorded; `replies` are handed
    /// out by `recv_udp` in order, each after its delay.
    struct Fake {
        now: u64,
        sends: Vec<usize>,
        replies: Vec<(u64, Vec<u8>, Option<usize>)>,
        tcp_answer: Option<Vec<u8>>,
        tcp_calls: usize,
    }

    impl Fake {
        fn new(replies: Vec<(u64, Vec<u8>, Option<usize>)>) -> Self {
            Self {
                now: 0,
                sends: Vec::new(),
                replies,
                tcp_answer: None,
                tcp_calls: 0,
            }
        }
    }

    impl Transport for Fake {
        fn send_udp(&mut self, i: usize, _q: &[u8]) {
            self.sends.push(i);
        }
        fn recv_udp(&mut self, buf: &mut [u8], ms: u64) -> Option<(usize, Option<usize>, bool)> {
            if self.replies.is_empty() {
                self.now += ms;
                return None;
            }
            let (delay, pkt, from) = self.replies.remove(0);
            if delay > ms {
                self.replies.insert(0, (delay - ms, pkt, from));
                self.now += ms;
                return None;
            }
            self.now += delay;
            buf[..pkt.len()].copy_from_slice(&pkt);
            Some((pkt.len(), from, false))
        }
        fn tcp(&mut self, _i: usize, _q: &[u8], buf: &mut [u8], _ms: u64) -> Option<usize> {
            self.tcp_calls += 1;
            let a = self.tcp_answer.clone()?;
            let keep = a.len().min(buf.len());
            buf[..keep].copy_from_slice(&a[..keep]);
            Some(a.len())
        }
        fn now_ms(&mut self) -> u64 {
            self.now
        }
    }

    const Q: &[u8] = b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00";

    fn reply(id: [u8; 2], flags2: u8, rcode: u8, ancount: u16) -> Vec<u8> {
        let mut r = vec![id[0], id[1], 0x81 | flags2, 0x80 | rcode];
        r.extend_from_slice(&[0, 1]);
        r.extend_from_slice(&ancount.to_be_bytes());
        r.extend_from_slice(&[0, 0, 0, 0]);
        r
    }

    fn two_servers() -> Servers {
        Servers {
            addrs: [SIN_ZERO; MAXNS],
            n: 2,
            timeout_ms: 5000,
            attempts: 2,
            tcp_only: false,
        }
    }

    #[test]
    fn test_exchange_takes_the_first_usable_answer() {
        let mut t = Fake::new(vec![
            (10, reply([0x99, 0x99], 0, 0, 1), Some(0)), // another query's
            (10, reply([0x12, 0x34], 0, 0, 1), None),    // not from our server
            (10, reply([0x12, 0x34], 0, REFUSED, 0), Some(0)),
            (10, reply([0x12, 0x34], 0, NXDOMAIN, 0), Some(1)),
        ]);
        let mut ans = [0u8; 512];
        assert_eq!(exchange(&mut t, &two_servers(), Q, &mut ans), Ok(12));
        assert_eq!(ans[3] & 0xf, NXDOMAIN);
        assert_eq!(t.sends, vec![0, 1], "every server, once");
    }

    #[test]
    fn test_exchange_resends_on_servfail_and_at_intervals() {
        let mut t = Fake::new(vec![
            (10, reply([0x12, 0x34], 0, SERVFAIL, 0), Some(1)),
            (3000, reply([0x12, 0x34], 0, 0, 2), Some(0)),
        ]);
        let mut ans = [0u8; 512];
        assert_eq!(exchange(&mut t, &two_servers(), Q, &mut ans), Ok(12));
        // Both at 0, server 1 again after its SERVFAIL, both again at 2500.
        assert_eq!(t.sends, vec![0, 1, 1, 0, 1]);
    }

    #[test]
    fn test_exchange_falls_back_to_tcp_on_truncation() {
        let mut t = Fake::new(vec![(5, reply([0x12, 0x34], 0x02, 0, 1), Some(0))]);
        let mut big = reply([0x12, 0x34], 0, 0, 9);
        big.resize(700, 7);
        t.tcp_answer = Some(big);
        let mut ans = [0u8; 512];
        assert_eq!(
            exchange(&mut t, &two_servers(), Q, &mut ans),
            Ok(700),
            "the whole length"
        );
        assert_eq!(t.tcp_calls, 1);
        assert_eq!(ans[7], 9);
    }

    #[test]
    fn test_exchange_errors() {
        let mut ans = [0u8; 512];
        let mut silent = Fake::new(vec![]);
        assert_eq!(
            exchange(&mut silent, &two_servers(), Q, &mut ans),
            Err(errno::ECONNREFUSED)
        );
        assert!(silent.now >= 5000);
        let mut refusing = Fake::new(vec![(1, reply([0x12, 0x34], 0, REFUSED, 0), Some(0))]);
        assert_eq!(
            exchange(&mut refusing, &two_servers(), Q, &mut ans),
            Err(errno::ETIMEDOUT)
        );
        let mut vc = Fake::new(vec![]);
        vc.tcp_answer = Some(reply([0x12, 0x34], 0, 0, 1));
        let sv = Servers {
            tcp_only: true,
            ..two_servers()
        };
        assert_eq!(exchange(&mut vc, &sv, Q, &mut ans), Ok(12));
        assert!(vc.sends.is_empty());
    }

    #[test]
    fn test_judge() {
        assert_eq!(judge(&reply([0, 0], 0, 0, 1)), Ok(()));
        assert_eq!(judge(&reply([0, 0], 0, 0, 0)), Err(crate::socket::NO_DATA));
        assert_eq!(
            judge(&reply([0, 0], 0, NXDOMAIN, 0)),
            Err(crate::socket::HOST_NOT_FOUND)
        );
        assert_eq!(
            judge(&reply([0, 0], 0, SERVFAIL, 0)),
            Err(crate::socket::TRY_AGAIN)
        );
        for rc in [FORMERR, NOTIMP, REFUSED, 9] {
            assert_eq!(
                judge(&reply([0, 0], 0, rc, 0)),
                Err(crate::socket::NO_RECOVERY)
            );
        }
    }

    // -- res_search --

    /// Record the names tried; answer the one named `hit`, if any.
    fn run_search(
        name: &[u8],
        ndots: usize,
        options: u64,
        domains: &[&[u8]],
        hit: Option<&str>,
        miss: i32,
    ) -> (Result<usize, i32>, Vec<String>) {
        let mut tried = Vec::new();
        let r = search_with(name, ndots, options, domains, |d| {
            let full = match d {
                None => {
                    String::from_utf8_lossy(name.strip_suffix(b".").unwrap_or(name)).into_owned()
                }
                Some(d) => format!(
                    "{}.{}",
                    String::from_utf8_lossy(name),
                    String::from_utf8_lossy(d)
                ),
            };
            tried.push(full.clone());
            if hit == Some(full.as_str()) {
                Ok(1)
            } else {
                Err((miss, false, false))
            }
        });
        (r, tried)
    }

    #[test]
    fn test_search_order() {
        let doms: &[&[u8]] = &[b"a.example", b"b.example"];
        let nf = crate::socket::HOST_NOT_FOUND;
        // No dots, ndots 1: the domains, then the name as it stands.
        let (r, t) = run_search(b"host", 1, RES_DEFAULT, doms, None, nf);
        assert_eq!(t, ["host.a.example", "host.b.example", "host"]);
        assert_eq!(r, Err(nf));
        // Enough dots: as it stands first.
        let (_, t) = run_search(b"x.y", 1, RES_DEFAULT, doms, None, nf);
        assert_eq!(t, ["x.y", "x.y.a.example", "x.y.b.example"]);
        // A trailing dot: only as it stands.
        let (_, t) = run_search(b"x.y.", 1, RES_DEFAULT, doms, None, nf);
        assert_eq!(t, ["x.y"]);
        // RES_DEFNAMES alone: one domain.
        let (_, t) = run_search(b"host", 1, RES_DEFNAMES, doms, None, nf);
        assert_eq!(t, ["host.a.example", "host"]);
        // RES_NOTLDQUERY: a dot-free name is never tried bare.
        let (_, t) = run_search(b"host", 1, RES_DEFAULT | RES_NOTLDQUERY, doms, None, nf);
        assert_eq!(t, ["host.a.example", "host.b.example"]);
        // The first answer wins.
        let (r, t) = run_search(b"host", 1, RES_DEFAULT, doms, Some("host.a.example"), nf);
        assert_eq!((r, t.len()), (Ok(1), 1));
    }

    #[test]
    fn test_search_verdicts() {
        let doms: &[&[u8]] = &[b"a.example", b"b.example"];
        // NO_DATA keeps searching, and is what is reported.
        let (r, t) = run_search(b"host", 1, RES_DEFAULT, doms, None, crate::socket::NO_DATA);
        assert_eq!((r, t.len()), (Err(crate::socket::NO_DATA), 3));
        // Anything else stops the domains, but the bare name is still tried.
        let (r, t) = run_search(
            b"host",
            1,
            RES_DEFAULT,
            doms,
            None,
            crate::socket::NO_RECOVERY,
        );
        assert_eq!(t, ["host.a.example", "host"]);
        assert_eq!(r, Err(crate::socket::NO_RECOVERY));
        // A refused connection gives up at once, as TRY_AGAIN.
        let r = search_with(b"host", 1, RES_DEFAULT, doms, |_| {
            Err((crate::socket::TRY_AGAIN, true, false))
        });
        assert_eq!(r, Err(crate::socket::TRY_AGAIN));
        // The root on the list stands for the bare name.
        let root: &[&[u8]] = &[b"."];
        let (_, t) = run_search(
            b"host",
            1,
            RES_DEFAULT,
            root,
            None,
            crate::socket::HOST_NOT_FOUND,
        );
        assert_eq!(t, ["host."]);
    }

    #[test]
    fn test_join() {
        let mut out = [0u8; MAXDNAME];
        assert_eq!(join(b"www", Some(b"example.com"), &mut out), Some(15));
        assert_eq!(&out[..15], b"www.example.com");
        assert_eq!(join(b"www.example.com.", None, &mut out), Some(15));
        assert_eq!(join(&[b'x'; 1020], Some(b"abcd"), &mut out), None);
    }

    // -- the entry points on the host --

    #[test]
    fn test_res_init_defaults_on_the_host() {
        // The host has no /etc/resolv.conf to read: glibc's defaults.
        assert_eq!(res_init(), 0);
        // SAFETY: this thread's state.
        let st = unsafe { &*__res_state() };
        assert_eq!(st.nscount, 1);
        assert_eq!(
            st.nsaddr_list[0].sin_addr.s_addr.to_ne_bytes(),
            [127, 0, 0, 1]
        );
        assert_eq!(
            (st.retrans, st.retry, st.ndots()),
            (RES_TIMEOUT, RES_DFLRETRY, 1)
        );
        assert_eq!(st.options & RES_DEFAULT, RES_DEFAULT);
        assert!(st.options & RES_INIT != 0);
    }

    #[test]
    fn test_res_send_arguments() {
        let _ = res_init();
        let mut ans = [0u8; 512];
        errno::set_errno(0);
        assert_eq!(res_send(Q.as_ptr(), 12, ans.as_mut_ptr(), 11), -1);
        assert_eq!(
            errno::get_errno(),
            errno::EINVAL,
            "a buffer shorter than a header"
        );
        errno::set_errno(0);
        assert_eq!(res_send(core::ptr::null(), 12, ans.as_mut_ptr(), 512), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        // SAFETY: this thread's state.
        unsafe { (*__res_state()).nscount = 0 };
        errno::set_errno(0);
        assert_eq!(res_send(Q.as_ptr(), 12, ans.as_mut_ptr(), 512), -1);
        assert_eq!(errno::get_errno(), errno::ESRCH, "no nameserver");
        let _ = res_init();
    }

    /// The host has no sockets: a query fails as a send failure does --
    /// TRY_AGAIN, with the socket's errno.
    #[test]
    fn test_res_query_without_a_network() {
        let _ = res_init();
        let mut ans = [0u8; 512];
        crate::socket::set_h_errno(0);
        assert_eq!(
            res_query(b"example.com\0".as_ptr(), C_IN, T_A, ans.as_mut_ptr(), 512),
            -1
        );
        assert_eq!(crate::socket::get_h_errno(), crate::socket::TRY_AGAIN);
        crate::socket::set_h_errno(0);
        errno::set_errno(0);
        assert_eq!(
            res_query(core::ptr::null(), C_IN, T_A, ans.as_mut_ptr(), 512),
            -1
        );
        assert_eq!(
            (crate::socket::get_h_errno(), errno::get_errno()),
            (NETDB_INTERNAL, errno::EFAULT)
        );
        // A name that cannot be sent is NO_RECOVERY, before any network.
        let mut name = vec![b'x'; 70];
        name.push(0);
        assert_eq!(
            res_query(name.as_ptr(), C_IN, T_A, ans.as_mut_ptr(), 512),
            -1
        );
        assert_eq!(crate::socket::get_h_errno(), crate::socket::NO_RECOVERY);
    }

    #[test]
    fn test_res_state_layout() {
        assert_eq!(size_of::<ResState>(), 568);
    }

    #[test]
    fn test_ns_get_put() {
        let mut b = [0u8; 4];
        ns_put16(0x1234, b.as_mut_ptr());
        assert_eq!(&b[..2], &[0x12, 0x34]);
        assert_eq!(ns_get16(b.as_ptr()), 0x1234);
        ns_put32(0xdead_beef, b.as_mut_ptr());
        assert_eq!(b, [0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(ns_get32(b.as_ptr()), 0xdead_beef);
        assert_eq!(ns_get16(core::ptr::null()), 0);
    }
}
