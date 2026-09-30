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
//! - **The reentrant forms** -- `res_ninit`, `res_nquery`, `res_nsearch`,
//!   `res_nquerydomain`, `res_nmkquery`, `res_nsend`, `res_nclose` -- do the
//!   same on a state of the caller's own, used as glibc uses it: as it is.
//!   A failure sets `h_errno` and the state's `res_h_errno` together, as
//!   glibc's `RES_SET_H_ERRNO` does, for these and `_res`'s alike.
//!
//! Not done: IPv6 nameservers (read, and skipped), `sortlist`, EDNS0,
//! `HOSTALIASES`, DNSSEC.  `_res` is one per process, as in musl; glibc's is
//! one per thread -- a threaded program wanting its own has the reentrant
//! forms.

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
/// Do not look names up under `ip6.int` (accepted; this resolver never
/// does). musl's header puts it in [`RES_DEFAULT`]; glibc 2.39's retired it.
pub const RES_NOIP6DOTINT: u64 = 0x0008_0000;
/// Never query a dot-free name as it stands: glibc's bit. (0x0010_0000
/// until 2026-09-30, which is `RES_USE_EDNS0` in both headers, so a program
/// asking for EDNS0 got this instead.)
pub const RES_NOTLDQUERY: u64 = 0x0100_0000;
/// Set the AD bit in queries.
pub const RES_TRUSTAD: u64 = 0x0400_0000;
/// musl's header's `RES_DEFAULT`, which a program here is compiled against:
/// glibc 2.39's with `RES_NOIP6DOTINT` besides. A fresh state starts with
/// glibc's ([`FRESH_OPTIONS`]).
pub const RES_DEFAULT: u64 = RES_RECURSE | RES_DEFNAMES | RES_DNSRCH | RES_NOIP6DOTINT;

/// The options a fresh state starts with: glibc 2.39's `RES_DEFAULT`, as
/// its `res_ninit` leaves them (`resolvn_oracle.txt`: `2c1` with
/// `RES_INIT`). Until 2026-09-30 a state started with musl's header's
/// [`RES_DEFAULT`] instead, whose `RES_NOIP6DOTINT` does nothing here or in
/// glibc but showed in `_res.options` as a number glibc's never is.
const FRESH_OPTIONS: u64 = RES_RECURSE | RES_DEFNAMES | RES_DNSRCH;

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
        options: FRESH_OPTIONS,
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

/// `res_ninit` -- read `resolv.conf` and the environment into the caller's
/// own resolver state `statp`, as [`res_init`] reads them into `_res`: 0.
/// Exported as `__res_ninit`, the name glibc's `<resolv.h>` (and
/// `posix/include/resolv.h`) turns `res_ninit` into. glibc asks for a
/// zeroed state and follows the pointers in one of garbage; this one is
/// overwritten whole.
///
/// # Safety
///
/// `statp` points to a writable `ResState`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __res_ninit(statp: *mut ResState) -> i32 {
    if statp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: the caller's contract.
    unsafe {
        statp.write(ResState::ZERO);
        load_state(&mut *statp);
    }
    0
}

/// `res_nclose` (exported as `__res_nclose`, as glibc's is) -- release what
/// `res_ninit` took for `statp`. This resolver keeps neither socket nor
/// memory in a state -- a query opens and closes its own -- so there is
/// nothing to release, and the state, `RES_INIT` still set as glibc leaves
/// it (`resolvn_oracle.txt`), stays usable. glibc's does not: a query on it
/// before `res_ninit` again crashes.
///
/// # Safety
///
/// `statp` is NULL or a `ResState`; it is not touched.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __res_nclose(_statp: *mut ResState) {}

// ---------------------------------------------------------------------------
// Names: glibc's ns_name_* functions
// ---------------------------------------------------------------------------

/// `dn` as a wire name, if it is printable ASCII and a name `ns_name_pton`
/// accepts: what all four of glibc's name checks ask first.  The wire
/// name's length.
fn printable_wire(dn: &[u8], wire: &mut [u8; MAXCDNAME]) -> Option<usize> {
    if !dn.iter().all(|&c| c > b' ' && c <= b'~') {
        return None;
    }
    name_pton(dn, wire).ok().map(|(n, _)| n)
}

/// Whether every label of the wire name `wire` is only letters, digits, `-`
/// and `_`: a host name's labels.
fn host_labels(wire: &[u8]) -> bool {
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

/// Whether the wire name's first label starts with `-`.
fn starts_with_hyphen(wire: &[u8]) -> bool {
    wire.first().is_some_and(|&l| l > 0) && wire.get(1) == Some(&b'-')
}

/// glibc's `res_hnok`: whether `dn` is a host name -- printable ASCII, a
/// name `ns_name_pton` accepts, and every label only letters, digits, `-`
/// and `_`, the first not starting with `-`.  What the DNS module asks of a
/// name an answer gives.
pub(crate) fn hnok(dn: &[u8]) -> bool {
    let mut wire = [0u8; MAXCDNAME];
    let Some(n) = printable_wire(dn, &mut wire) else {
        return false;
    };
    let wire = wire.get(..n).unwrap_or(&[]);
    !starts_with_hyphen(wire) && host_labels(wire)
}

/// glibc's `res_ownok`: [`hnok`], but a first label of `*` alone is let
/// through -- a wildcard record's owner.
fn ownok(dn: &[u8]) -> bool {
    let mut wire = [0u8; MAXCDNAME];
    let Some(n) = printable_wire(dn, &mut wire) else {
        return false;
    };
    let wire = wire.get(..n).unwrap_or(&[]);
    if starts_with_hyphen(wire) {
        return false;
    }
    if wire.starts_with(&[1, b'*']) {
        host_labels(wire.get(2..).unwrap_or(&[]))
    } else {
        host_labels(wire)
    }
}

/// glibc's `res_mailok`: a mailbox as DNS writes one -- the first label
/// anything printable, and at least one more after it, which are a host
/// name's; `.` alone passes.
fn mailok(dn: &[u8]) -> bool {
    let mut wire = [0u8; MAXCDNAME];
    let Some(n) = printable_wire(dn, &mut wire) else {
        return false;
    };
    let wire = wire.get(..n).unwrap_or(&[]);
    let Some(&first) = wire.first() else {
        return false;
    };
    if first == 0 {
        return true;
    }
    let tail = wire.get(1 + usize::from(first)..).unwrap_or(&[]);
    tail.first().is_some_and(|&l| l != 0) && host_labels(tail)
}

/// The C string at `dn`, or `None` for NULL.
fn c_name<'a>(dn: *const u8) -> Option<&'a [u8]> {
    if dn.is_null() {
        return None;
    }
    // SAFETY: a non-NULL `dn` is a NUL-terminated string, per the contract.
    Some(unsafe { core::slice::from_raw_parts(dn, crate::string::strlen(dn)) })
}

/// Whether `dn` is a host name: 1 or 0.  See [`hnok`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_hnok(dn: *const u8) -> i32 {
    i32::from(c_name(dn).is_some_and(hnok))
}

/// Whether `dn` may own a host's records: a host name, or one whose first
/// label is `*`.  1 or 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_ownok(dn: *const u8) -> i32 {
    i32::from(c_name(dn).is_some_and(ownok))
}

/// Whether `dn` is a mailbox as DNS writes one (`SOA` and `RP` records):
/// a first label of anything printable, then a host name.  1 or 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_mailok(dn: *const u8) -> i32 {
    i32::from(c_name(dn).is_some_and(mailok))
}

/// Whether `dn` is a domain name at all: printable ASCII that
/// `ns_name_pton` accepts.  1 or 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_dnok(dn: *const u8) -> i32 {
    let mut wire = [0u8; MAXCDNAME];
    i32::from(c_name(dn).is_some_and(|d| printable_wire(d, &mut wire).is_some()))
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
    // Not `unwrap_or(srcp - at)`: that subtracts even when a pointer set
    // `len`, and a pointer back into the message leaves `srcp` behind `at`
    // -- an overflow, a panic in a debug build, in every compressed answer.
    Ok(match len {
        Some(l) => l,
        None => srcp - at,
    })
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
    match skip_len(s).map(i32::try_from) {
        Ok(Ok(n)) => n,
        _ => name_fail(),
    }
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
// ns_name_ntop / ns_name_pton / ns_name_unpack / ns_name_pack /
// ns_name_compress / ns_name_skip
// ---------------------------------------------------------------------------

/// The length of the uncompressed wire name at `src`, found by walking its
/// labels -- as glibc's functions read it, never past the name -- or `Err`
/// at a label no uncompressed name can hold (a compression pointer, an
/// extended label type).
///
/// # Safety
///
/// `src` points to a wire name, readable up to its terminating zero label.
unsafe fn wire_len(src: *const u8) -> Result<usize, ()> {
    let mut at = 0usize;
    loop {
        // SAFETY: the caller's contract: every byte up to the root label.
        let n = usize::from(unsafe { *src.add(at) });
        at += 1;
        if n == 0 {
            return Ok(at);
        }
        if n >= 64 {
            // The label byte is part of the name: ntop refuses it itself.
            return Ok(at);
        }
        at += n;
    }
}

/// The value glibc's `ns_name_*` functions answer for `Err`: -1, with
/// `EMSGSIZE`.
fn name_fail() -> i32 {
    errno::set_errno(errno::EMSGSIZE);
    -1
}

/// `ns_name_ntop` -- the uncompressed wire name at `src` as NUL-terminated
/// text in `dst` (`dstsiz` bytes): labels joined by `.`, `"`, `.`, `;`,
/// `\`, `(`, `)`, `@` and `$` escaped with a backslash, bytes outside
/// `!`..`~` as `\DDD`, the root as `.`.  The bytes written with the NUL, or
/// -1 with `EMSGSIZE` for too small a `dst` or a label no uncompressed name
/// holds (`nsname_oracle.txt`).
///
/// # Safety
///
/// `src` is a wire name readable to its end; `dst` holds `dstsiz` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_name_ntop(src: *const u8, dst: *mut u8, dstsiz: usize) -> i32 {
    if src.is_null() || dst.is_null() {
        return name_fail();
    }
    // SAFETY: the caller's contract.
    let result = unsafe { wire_len(src) }.and_then(|len| {
        // SAFETY: as above: `len` bytes of `src`, `dstsiz` of `dst`.
        let (from, to) = unsafe {
            (
                core::slice::from_raw_parts(src, len),
                core::slice::from_raw_parts_mut(dst, dstsiz),
            )
        };
        name_ntop(from, to)
    });
    match result.map(i32::try_from) {
        Ok(Ok(n)) => n,
        _ => name_fail(),
    }
}

/// `ns_name_pton` -- the text name `src` as an uncompressed wire name in
/// `dst` (`dstsiz` bytes), `\.` and `\DDD` unescaped.  1 when the text
/// was fully qualified (ended in `.`), 0 when not -- the empty text is the
/// root, unqualified -- or -1 with `EMSGSIZE`: an empty label, one over 63
/// bytes, a name over 255, a bad escape, too small a `dst`.
///
/// # Safety
///
/// `src` is a NUL-terminated string; `dst` holds `dstsiz` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_name_pton(src: *const u8, dst: *mut u8, dstsiz: usize) -> i32 {
    let Some(text) = c_name(src) else {
        return name_fail();
    };
    if dst.is_null() {
        return name_fail();
    }
    // SAFETY: the caller's contract: `dst` holds `dstsiz` bytes.
    let to = unsafe { core::slice::from_raw_parts_mut(dst, dstsiz) };
    match name_pton(text, to) {
        Ok((_, qualified)) => i32::from(qualified),
        Err(()) => name_fail(),
    }
}

/// `ns_name_unpack` -- the compressed name at `src` in the message `[msg,
/// eom)`, compression pointers followed (a forward one too; a loop is
/// refused), into `dst` (`dstsiz` bytes) uncompressed.  The bytes the name
/// occupies at `src`, or -1 with `EMSGSIZE`.
///
/// # Safety
///
/// `[msg, eom)` is a readable message and `src` inside it; `dst` holds
/// `dstsiz` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_name_unpack(
    msg: *const u8,
    eom: *const u8,
    src: *const u8,
    dst: *mut u8,
    dstsiz: usize,
) -> i32 {
    let result = (|| {
        if msg.is_null() || dst.is_null() {
            return Err(());
        }
        let len = (eom as usize).checked_sub(msg as usize).ok_or(())?;
        let at = (src as usize).checked_sub(msg as usize).ok_or(())?;
        // SAFETY: the caller's contract.
        let (whole, to) = unsafe {
            (
                core::slice::from_raw_parts(msg, len),
                core::slice::from_raw_parts_mut(dst, dstsiz),
            )
        };
        name_unpack(whole, at, to)
    })();
    match result.map(i32::try_from) {
        Ok(Ok(n)) => n,
        _ => name_fail(),
    }
}

/// `ns_name_pack` -- the uncompressed wire name at `src` into `dst`
/// (`dstsiz` bytes), compressed against the names `dnptrs` lists, and added
/// to the list when `lastdnptr` leaves it room -- glibc's pointer table: the
/// message first, then the names in it, NULL-terminated.  The bytes
/// written, or -1 with `EMSGSIZE`.
///
/// # Safety
///
/// `src` is a wire name readable to its end; `dst` holds `dstsiz` bytes;
/// `dnptrs` is NULL or a table as above, whose array ends at `lastdnptr`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_name_pack(
    src: *const u8,
    dst: *mut u8,
    dstsiz: i32,
    dnptrs: *mut *const u8,
    lastdnptr: *mut *const u8,
) -> i32 {
    let result = (|| {
        if src.is_null() || dst.is_null() {
            return Err(());
        }
        let cap = usize::try_from(dstsiz).map_err(|_| ())?;
        // SAFETY: the caller's contract.
        let len = unsafe { wire_len(src) }?;
        // SAFETY: as above: `len` bytes of `src`.
        let from = unsafe { core::slice::from_raw_parts(src, len) };
        // SAFETY: the caller's contract for `dst`, `dnptrs` and `lastdnptr`.
        unsafe { name_pack(from, dst, cap, dnptrs.cast(), lastdnptr.cast()) }
    })();
    match result.map(i32::try_from) {
        Ok(Ok(n)) => n,
        _ => name_fail(),
    }
}

/// `ns_name_compress` -- the text name `src` into `dst` (`dstsiz` bytes),
/// through `ns_name_pton` and `ns_name_pack`: [`dn_comp`], with a `size_t`
/// size.
///
/// # Safety
///
/// As [`ns_name_pack`], with `src` a NUL-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_name_compress(
    src: *const u8,
    dst: *mut u8,
    dstsiz: usize,
    dnptrs: *mut *const u8,
    lastdnptr: *mut *const u8,
) -> i32 {
    let result = (|| {
        let text = c_name(src).ok_or(())?;
        if dst.is_null() {
            return Err(());
        }
        let mut tmp = [0u8; MAXCDNAME];
        name_pton(text, &mut tmp)?;
        // SAFETY: the caller's contract for `dst`, `dnptrs` and `lastdnptr`.
        unsafe { name_pack(&tmp, dst, dstsiz, dnptrs.cast(), lastdnptr.cast()) }
    })();
    match result.map(i32::try_from) {
        Ok(Ok(n)) => n,
        _ => name_fail(),
    }
}

/// How many bytes the compressed name at the start of `s` occupies -- a
/// pointer ends it, as two bytes, without being followed -- or `Err` if it
/// runs past `s` or holds a label that is neither length nor pointer.
fn skip_len(s: &[u8]) -> Result<usize, ()> {
    let mut cp = 0usize;
    while let Some(&n) = s.get(cp) {
        cp += 1;
        if n == 0 {
            return Ok(cp);
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
                return Ok(cp + 1);
            }
            _ => break,
        }
    }
    Err(())
}

/// `ns_name_skip` -- move `*ptrptr` past the compressed name it points at,
/// which must end before `eom`: 0, or -1 with `EMSGSIZE` and `*ptrptr` where
/// it was.
///
/// # Safety
///
/// `*ptrptr` and `eom` bound a readable range.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_name_skip(ptrptr: *mut *const u8, eom: *const u8) -> i32 {
    if ptrptr.is_null() {
        return name_fail();
    }
    // SAFETY: the caller's contract.
    let start = unsafe { *ptrptr };
    if start.is_null() {
        return name_fail();
    }
    let avail = (eom as usize).saturating_sub(start as usize);
    // SAFETY: the caller's contract: `[start, eom)` is readable.
    let s = unsafe { core::slice::from_raw_parts(start, avail) };
    match skip_len(s) {
        Ok(n) => {
            // SAFETY: `n <= avail`: the pointer stays in the range.
            unsafe { *ptrptr = start.add(n) };
            0
        }
        Err(()) => name_fail(),
    }
}

/// `ns_name_ntol` -- the uncompressed wire name at `src`, its labels in
/// ASCII lower case, into `dst` (`dstsiz` bytes): the bytes written, the
/// root label counted, or -1 with `EMSGSIZE` for a compression pointer, a
/// label over 63 bytes, or too small a `dst` -- glibc's order kept, each
/// label's length byte written before its size is judged, so a failure
/// leaves the labels before it (`nsutil_oracle.txt`).
///
/// # Safety
///
/// `src` is a wire name readable to its root label or to the byte that
/// stops it; `dst` holds `dstsiz` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_name_ntol(src: *const u8, dst: *mut u8, dstsiz: usize) -> i32 {
    if src.is_null() || dst.is_null() || dstsiz == 0 {
        return name_fail();
    }
    // SAFETY: the caller's contract: `dst` holds `dstsiz` bytes.
    let out = unsafe { core::slice::from_raw_parts_mut(dst, dstsiz) };
    let mut cp = 0usize;
    let mut dn = 0usize;
    loop {
        // SAFETY: the caller's contract: `src` is readable to the byte that
        // ends the name, and each byte read is before that one.
        let n = unsafe { *src.add(cp) };
        cp += 1;
        if n == 0 {
            break;
        }
        if n & 0xc0 == 0xc0 {
            return name_fail();
        }
        // The previous label's check left room for this length byte.
        out[dn] = n;
        dn += 1;
        if n > 63 {
            return name_fail();
        }
        let l = usize::from(n);
        if dn + l >= dstsiz {
            return name_fail();
        }
        for _ in 0..l {
            // SAFETY: as above, a label byte before the end of the name.
            out[dn] = unsafe { *src.add(cp) }.to_ascii_lowercase();
            cp += 1;
            dn += 1;
        }
    }
    out[dn] = 0;
    i32::try_from(dn + 1).unwrap_or(i32::MAX)
}

/// `ns_name_rollback` -- forget the names `dn_comp` (or `ns_name_pack`)
/// recorded at or past `src`: the first entry of `dnptrs`, before
/// `lastdnptr` and before a NULL one, that points at or past `src` becomes
/// NULL, ending the table there -- a message cut back to `src` then leaves
/// no pointer into what it lost. A NULL table is nothing to do.
///
/// # Safety
///
/// `dnptrs` is NULL or points into a table of `const u_char *` that
/// `lastdnptr` bounds.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_name_rollback(
    src: *const u8,
    dnptrs: *mut *const u8,
    lastdnptr: *mut *const u8,
) {
    if dnptrs.is_null() {
        return;
    }
    let mut p = dnptrs;
    while (p as usize) < (lastdnptr as usize) {
        // SAFETY: `p` is before `lastdnptr`, in the caller's table.
        let entry = unsafe { *p };
        if entry.is_null() {
            return;
        }
        if entry as usize >= src as usize {
            // SAFETY: as above.
            unsafe { *p = core::ptr::null() };
            return;
        }
        // SAFETY: as above: the next entry, or `lastdnptr` itself.
        p = unsafe { p.add(1) };
    }
}

// ---------------------------------------------------------------------------
// ns_get16 / ns_get32 / ns_put16 / ns_put32
// ---------------------------------------------------------------------------

/// `ns_get16` -- a 16-bit value in network byte order, as the `unsigned`
/// the header declares.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_get16(src: *const u8) -> u32 {
    if src.is_null() {
        return 0;
    }
    // SAFETY: caller guarantees at least 2 bytes.
    let b = unsafe { core::ptr::read_unaligned(src.cast::<[u8; 2]>()) };
    u32::from(u16::from_be_bytes(b))
}

/// `ns_get32` -- a 32-bit value in network byte order, as the `unsigned
/// long` the header declares.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_get32(src: *const u8) -> u64 {
    if src.is_null() {
        return 0;
    }
    // SAFETY: caller guarantees at least 4 bytes.
    let b = unsafe { core::ptr::read_unaligned(src.cast::<[u8; 4]>()) };
    u64::from(u32::from_be_bytes(b))
}

/// `ns_put16` -- the low 16 bits of `val` in network byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_put16(val: u32, dst: *mut u8) {
    if dst.is_null() {
        return;
    }
    // The header's `unsigned`; the field is 16 bits, as in C's `*cp++ = s`.
    #[allow(clippy::cast_possible_truncation)]
    let v = val as u16;
    // SAFETY: caller guarantees at least 2 bytes.
    unsafe { core::ptr::write_unaligned(dst.cast::<[u8; 2]>(), v.to_be_bytes()) };
}

/// `ns_put32` -- the low 32 bits of `val` in network byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_put32(val: u64, dst: *mut u8) {
    if dst.is_null() {
        return;
    }
    // The header's `unsigned long`; the field is 32 bits.
    #[allow(clippy::cast_possible_truncation)]
    let v = val as u32;
    // SAFETY: caller guarantees at least 4 bytes.
    unsafe { core::ptr::write_unaligned(dst.cast::<[u8; 4]>(), v.to_be_bytes()) };
}

// ---------------------------------------------------------------------------
// ns_initparse / ns_parserr / ns_skiprr / ns_name_uncompress
// ---------------------------------------------------------------------------

/// The longest name `ns_parserr` writes, as text: `NS_MAXDNAME`.
pub const NS_MAXDNAME: usize = 1025;

/// `ns_sect`: the question section (`ns_s_qd`, also `ns_s_zn`).
pub const NS_S_QD: i32 = 0;
/// The answer section (`ns_s_an`, also `ns_s_pr`).
pub const NS_S_AN: i32 = 1;
/// The authority section (`ns_s_ns`, also `ns_s_ud`).
pub const NS_S_NS: i32 = 2;
/// The additional section (`ns_s_ar`).
pub const NS_S_AR: i32 = 3;
/// One past the last section (`ns_s_max`): the handle between parses.
pub const NS_S_MAX: i32 = 4;

/// A message taken apart for [`ns_parserr`] (`ns_msg`). The C header's
/// field names begin with `_`; the layout is what matters, and
/// `abi_layout.rs` holds it to musl's.
#[repr(C)]
pub struct NsMsg {
    /// The message, and its end.
    pub msg: *const u8,
    /// One past the message's last byte.
    pub eom: *const u8,
    /// The header's id.
    pub id: u16,
    /// The header's flags word.
    pub flags: u16,
    /// Each section's record count.
    pub counts: [u16; 4],
    /// Each section's first record, NULL for an empty section.
    pub sections: [*const u8; 4],
    /// The section the next record is read from; `NS_S_MAX` for none.
    pub sect: i32,
    /// The number of that record in its section; -1 for none.
    pub rrnum: i32,
    /// Where that record begins.
    pub msg_ptr: *const u8,
}

/// One record, as [`ns_parserr`] fills it (`ns_rr`).
#[repr(C)]
pub struct NsRr {
    /// The owner name, as text, NUL-terminated ("" for the root).
    pub name: [u8; NS_MAXDNAME],
    /// The record type.
    pub type_: u16,
    /// The record class.
    pub rr_class: u16,
    /// The time to live; 0 for a question.
    pub ttl: u32,
    /// The length of `rdata`; 0 for a question.
    pub rdlength: u16,
    /// The record's data, in the message; NULL for a question.
    pub rdata: *const u8,
}

/// Where one of the header's flags lives in its flags word (`struct
/// _ns_flagdata`), for the `ns_msg_getflag` macro.
#[repr(C)]
pub struct NsFlagData {
    /// The flag's bits.
    pub mask: i32,
    /// How far to shift them down.
    pub shift: i32,
}

/// The header's flags, in `ns_flag`'s order -- `qr`, `opcode`, `aa`, `tc`,
/// `rd`, `ra`, `z`, `ad`, `cd`, `rcode` -- and six unused: the table
/// `ns_msg_getflag` reads.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static _ns_flagdata: [NsFlagData; 16] = {
    const fn f(mask: i32, shift: i32) -> NsFlagData {
        NsFlagData { mask, shift }
    }
    [
        f(0x8000, 15),
        f(0x7800, 11),
        f(0x0400, 10),
        f(0x0200, 9),
        f(0x0100, 8),
        f(0x0080, 7),
        f(0x0040, 6),
        f(0x0020, 5),
        f(0x0010, 4),
        f(0x000f, 0),
        f(0, 0),
        f(0, 0),
        f(0, 0),
        f(0, 0),
        f(0, 0),
        f(0, 0),
    ]
};

/// Fail with `err`: -1, and `errno` set.
fn ns_fail(err: i32) -> i32 {
    errno::set_errno(err);
    -1
}

/// The big-endian 16-bit value `off` bytes past `p`.
///
/// # Safety
///
/// `p + off` and the byte after it are readable.
unsafe fn get16_at(p: *const u8, off: usize) -> u16 {
    // SAFETY: the caller's contract.
    u16::from_be_bytes(unsafe { core::ptr::read_unaligned(p.add(off).cast::<[u8; 2]>()) })
}

/// Whether `n` bytes from `off` bytes past `from` reach past `eom`: glibc's
/// `ptr + n > eom`, without forming the pointer.
fn past(from: *const u8, off: usize, n: usize, eom: *const u8) -> bool {
    let room = (eom as usize).saturating_sub(from as usize);
    off.checked_add(n).is_none_or(|end| end > room)
}

/// `ns_skiprr` -- the bytes `count` records of `section` take from `ptr`
/// (a question is a name and four bytes; any other record a name, ten bytes
/// and its data), or -1 with `EMSGSIZE` when they run past `eom` or a name
/// is malformed. A `count` of 0 or less is 0 bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_skiprr(ptr: *const u8, eom: *const u8, section: i32, count: i32) -> i32 {
    let mut off = 0usize;
    for _ in 0..count.max(0) {
        let Ok(b) = usize::try_from(dn_skipname(ptr.wrapping_add(off), eom)) else {
            return ns_fail(errno::EMSGSIZE);
        };
        // The name, then the type and class.
        off = off.saturating_add(b).saturating_add(4);
        if section != NS_S_QD {
            // The TTL and the data's length must be there to read.
            if past(ptr, off, 6, eom) {
                return ns_fail(errno::EMSGSIZE);
            }
            // SAFETY: the six bytes at `off` are before `eom` (checked).
            let rdlength = unsafe { get16_at(ptr, off + 4) };
            off = off.saturating_add(6).saturating_add(usize::from(rdlength));
        }
    }
    if past(ptr, off, 0, eom) {
        return ns_fail(errno::EMSGSIZE);
    }
    i32::try_from(off).unwrap_or_else(|_| ns_fail(errno::EMSGSIZE))
}

/// Put `h` at the start of `sect`, or between parses for `NS_S_MAX`
/// (glibc's `setsection`).
fn set_section(h: &mut NsMsg, sect: i32) {
    h.sect = sect;
    match usize::try_from(sect).ok().and_then(|s| h.sections.get(s)) {
        Some(&start) if sect != NS_S_MAX => {
            h.rrnum = 0;
            h.msg_ptr = start;
        }
        _ => {
            h.rrnum = -1;
            h.msg_ptr = core::ptr::null();
        }
    }
}

/// `ns_initparse` -- take the `msglen`-byte message at `msg` apart into
/// `handle`: its header, and where each section's records begin. 0, or -1
/// with `EMSGSIZE` when the header is short, a section's records run past
/// the end or are malformed, or bytes are left over after the last. A NULL
/// `handle`, or a NULL `msg` with a header to read, is `EFAULT` (§303).
///
/// # Safety
///
/// `msg` is readable for `msglen` bytes; `handle` is NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_initparse(msg: *const u8, msglen: i32, handle: *mut NsMsg) -> i32 {
    if handle.is_null() {
        return ns_fail(errno::EFAULT);
    }
    // A negative length puts the end before the start: nothing can be read.
    let len = usize::try_from(msglen).unwrap_or(0);
    let eom = msg.wrapping_add(len);
    // SAFETY: non-null, the caller's to fill.
    let h = unsafe { &mut *handle };
    h.msg = msg;
    // glibc's `msg + msglen`, before the start for a negative length.
    h.eom = msg.wrapping_offset(isize::try_from(msglen).unwrap_or(0));
    if len < 12 {
        return ns_fail(errno::EMSGSIZE);
    }
    if msg.is_null() {
        return ns_fail(errno::EFAULT);
    }
    // SAFETY: the twelve header bytes are inside the message.
    unsafe {
        h.id = get16_at(msg, 0);
        h.flags = get16_at(msg, 2);
        for (i, count) in h.counts.iter_mut().enumerate() {
            *count = get16_at(msg, 4 + 2 * i);
        }
    }
    let mut off = 12usize;
    for i in 0..4usize {
        let count = h.counts.get(i).copied().unwrap_or(0);
        let slot = h.sections.get_mut(i);
        if count == 0 {
            if let Some(s) = slot {
                *s = core::ptr::null();
            }
            continue;
        }
        let at = msg.wrapping_add(off);
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let b = ns_skiprr(at, eom, i as i32, i32::from(count));
        let Ok(b) = usize::try_from(b) else {
            return -1; // ns_skiprr's errno
        };
        if let Some(s) = slot {
            *s = at;
        }
        off = off.saturating_add(b);
    }
    if off != len {
        return ns_fail(errno::EMSGSIZE);
    }
    set_section(h, NS_S_MAX);
    0
}

/// `ns_parserr` -- record `rrnum` of `section` into `rr`, as glibc 2.39's
/// reads it: 0, or -1 with `errno`. `rrnum` -1 is the handle's next record;
/// a section that is none, or a record number outside the section, is
/// `ENODEV`; a record that runs past the end, or a name that cannot be
/// expanded, `EMSGSIZE`. The handle keeps its place, so reading a section
/// in order walks it once; after the last record it stays at the section's
/// end (glibc's `++rrnum > count` never moves it on). A NULL `handle` or
/// `rr` is `EFAULT` (§303).
///
/// # Safety
///
/// `handle` is NULL or one `ns_initparse` filled, whose message is still
/// there; `rr` is NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_parserr(
    handle: *mut NsMsg,
    section: i32,
    rrnum: i32,
    rr: *mut NsRr,
) -> i32 {
    if handle.is_null() || rr.is_null() {
        return ns_fail(errno::EFAULT);
    }
    // SAFETY: non-null; the caller's.
    let (h, rr) = unsafe { (&mut *handle, &mut *rr) };
    let Some(count) = usize::try_from(section)
        .ok()
        .filter(|&s| s < 4)
        .and_then(|s| h.counts.get(s).copied())
    else {
        return ns_fail(errno::ENODEV);
    };
    if section != h.sect {
        set_section(h, section);
    }
    let rrnum = if rrnum == -1 { h.rrnum } else { rrnum };
    if rrnum < 0 || rrnum >= i32::from(count) {
        return ns_fail(errno::ENODEV);
    }
    if rrnum < h.rrnum {
        set_section(h, section);
    }
    if rrnum > h.rrnum {
        let Ok(b) = usize::try_from(ns_skiprr(h.msg_ptr, h.eom, section, rrnum - h.rrnum)) else {
            return -1; // ns_skiprr's errno
        };
        h.msg_ptr = h.msg_ptr.wrapping_add(b);
        h.rrnum = rrnum;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let b = dn_expand(
        h.msg,
        h.eom,
        h.msg_ptr,
        rr.name.as_mut_ptr(),
        NS_MAXDNAME as i32,
    );
    let Ok(b) = usize::try_from(b) else {
        return -1; // dn_expand's EMSGSIZE
    };
    // The handle moves on as each field is read, and a field is stored as
    // soon as it is read, before the next bounds check -- glibc's order, so
    // a record that fails half-way leaves what glibc's would.
    h.msg_ptr = h.msg_ptr.wrapping_add(b);
    if past(h.msg_ptr, 0, 4, h.eom) {
        return ns_fail(errno::EMSGSIZE);
    }
    // SAFETY: the four bytes at `msg_ptr` are before the end (checked).
    unsafe {
        rr.type_ = get16_at(h.msg_ptr, 0);
        rr.rr_class = get16_at(h.msg_ptr, 2);
    }
    h.msg_ptr = h.msg_ptr.wrapping_add(4);
    if section == NS_S_QD {
        rr.ttl = 0;
        rr.rdlength = 0;
        rr.rdata = core::ptr::null();
    } else {
        if past(h.msg_ptr, 0, 6, h.eom) {
            return ns_fail(errno::EMSGSIZE);
        }
        // SAFETY: the six bytes at `msg_ptr` are before the end (checked).
        unsafe {
            rr.ttl = u32::from_be_bytes(core::ptr::read_unaligned(h.msg_ptr.cast::<[u8; 4]>()));
            rr.rdlength = get16_at(h.msg_ptr, 4);
        }
        h.msg_ptr = h.msg_ptr.wrapping_add(6);
        if past(h.msg_ptr, 0, usize::from(rr.rdlength), h.eom) {
            return ns_fail(errno::EMSGSIZE);
        }
        rr.rdata = h.msg_ptr;
        h.msg_ptr = h.msg_ptr.wrapping_add(usize::from(rr.rdlength));
    }
    h.rrnum = h.rrnum.saturating_add(1);
    if h.rrnum > i32::from(count) {
        set_section(h, section.saturating_add(1));
    }
    0
}

/// `ns_name_uncompress` -- the compressed name at `src`, in the message
/// `[msg, eom)`, as text in `dst` (`dstsiz` bytes): the bytes the name
/// occupies at `src`, or -1 with `EMSGSIZE`. The root is ".", unlike
/// [`dn_expand`]'s "".
///
/// # Safety
///
/// `[msg, eom)` is readable and `src` inside it; `dst` is writable for
/// `dstsiz` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_name_uncompress(
    msg: *const u8,
    eom: *const u8,
    src: *const u8,
    dst: *mut u8,
    dstsiz: usize,
) -> i32 {
    let result = (|| {
        if msg.is_null() || dst.is_null() {
            return Err(());
        }
        let msg_len = (eom as usize).checked_sub(msg as usize).ok_or(())?;
        let at = (src as usize).checked_sub(msg as usize).ok_or(())?;
        // SAFETY: the caller's contract: `[msg, eom)` is the message.
        let whole = unsafe { core::slice::from_raw_parts(msg, msg_len) };
        let mut wire = [0u8; MAXCDNAME];
        let n = name_unpack(whole, at, &mut wire)?;
        // No name's text is longer than four bytes for each of its 255,
        // so a bigger buffer is only ever used this far.
        let cap = dstsiz.min(4 * MAXCDNAME + 2);
        // SAFETY: the caller's contract: `dst` holds `dstsiz` bytes.
        let out = unsafe { core::slice::from_raw_parts_mut(dst, cap) };
        name_ntop(&wire, out)?;
        i32::try_from(n).map_err(|_| ())
    })();
    result.unwrap_or_else(|()| ns_fail(errno::EMSGSIZE))
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
    datalen: i32,
    newrr: *const u8,
    buf: *mut u8,
    buflen: i32,
) -> i32 {
    // SAFETY: `_res`, initialised.
    unsafe {
        res_nmkquery(
            global(),
            op,
            dname,
            class,
            type_,
            data,
            datalen,
            newrr,
            buf,
            buflen,
        )
    }
}

/// `res_nmkquery` -- [`res_mkquery`] with the caller's own resolver state
/// `statp`: its options decide the RD and AD bits. Used as it is, one
/// `res_ninit` has not seen builds a query too, as glibc's does
/// (`resolvn_oracle.txt`).
///
/// # Safety
///
/// `statp` is a `ResState` the caller owns; the rest is as for
/// [`res_mkquery`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn res_nmkquery(
    statp: *mut ResState,
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
    if statp.is_null() || buf.is_null() || len < HFIXEDSZ || dname.is_null() {
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
    // SAFETY: the caller's state, only read.
    match mkquery(unsafe { &*statp }, op, name, class, type_, data, out) {
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
    // glibc's `__res_context_send`: no nameserver is ESRCH, before any
    // socket -- a state `res_ninit` has not seen has none.
    if st.nscount <= 0 {
        return Err(errno::ESRCH);
    }
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
    #[cfg(test)]
    if let Some(r) = tests::scripted_exchange(&sv, q, answer) {
        return r;
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
    // SAFETY: `_res`, initialised.
    unsafe { res_nsend(global(), msg, msglen, answer, anslen) }
}

/// `res_nsend` -- [`res_send`] with the caller's own resolver state
/// `statp`, used as it is: one `res_ninit` has not seen has no nameserver,
/// and the send is `ESRCH` (`resolvn_oracle.txt`). `h_errno` is left alone,
/// as glibc leaves it.
///
/// # Safety
///
/// `statp` is a `ResState` the caller owns, zeroed or from `res_ninit`; the
/// buffers are as for [`res_send`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn res_nsend(
    statp: *mut ResState,
    msg: *const u8,
    msglen: i32,
    answer: *mut u8,
    anslen: i32,
) -> i32 {
    if statp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: the caller's state, only read while the send runs.
    let st = unsafe { &*statp };
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

/// Set `h_errno` and the state's own `res_h_errno` together, as glibc's
/// `RES_SET_H_ERRNO` does: a caller of the reentrant functions reads the
/// second, and everyone the first.
///
/// # Safety
///
/// `statp` points to a live `ResState` that nothing is borrowing.
unsafe fn set_herr(statp: *mut ResState, h: i32) {
    // SAFETY: the caller's contract; the one field is written.
    unsafe { core::ptr::addr_of_mut!((*statp).res_h_errno).write(h) };
    crate::socket::set_h_errno(h);
}

/// `_res`, initialised if nothing has initialised it yet, as the pointer the
/// functions that take a state are given.
fn global() -> *mut ResState {
    let st: *mut ResState = state();
    st
}

/// A query's answer as the C functions give it: its length, or -1 with the
/// `h_errno` in both places.
///
/// # Safety
///
/// As [`set_herr`].
unsafe fn finish(statp: *mut ResState, r: Result<usize, i32>) -> i32 {
    match r {
        Ok(n) => i32::try_from(n).unwrap_or(i32::MAX),
        Err(h) => {
            // SAFETY: the caller's contract.
            unsafe { set_herr(statp, h) };
            -1
        }
    }
}

/// A query for `name`: `Ok` with the answer's length, else `Err` with the
/// `h_errno` it comes to (see [`judge`]) -- the send's own failure left in
/// `errno`.
fn query(
    st: &ResState,
    name: &[u8],
    class: i32,
    type_: i32,
    answer: &mut [u8],
) -> Result<usize, i32> {
    let mut q = [0u8; HFIXEDSZ + QFIXEDSZ + MAXCDNAME + 1];
    let Ok(ql) = mkquery(st, QUERY, name, class, type_, None, &mut q) else {
        return Err(crate::socket::NO_RECOVERY);
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
            Err(crate::socket::TRY_AGAIN)
        }
        Ok(n) => verdict.map(|()| n),
    }
}

/// The caller's name and answer buffer, or `None` for a NULL where glibc
/// would fault -- `EFAULT`, and the caller sets `h_errno` `NETDB_INTERNAL`.
fn args<'a>(name: *const u8, answer: *mut u8, anslen: i32) -> Option<(&'a [u8], &'a mut [u8])> {
    let len = usize::try_from(anslen).unwrap_or(0);
    if name.is_null() || (answer.is_null() && len > 0) {
        errno::set_errno(errno::EFAULT);
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
    // SAFETY: `_res`, initialised.
    unsafe { res_nquery(global(), dname, class, type_, answer, anslen) }
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

/// `res_nquery` -- [`res_query`] with the caller's own resolver state
/// `statp`, glibc's reentrant form. The state is used as it is: one
/// `res_ninit` has not seen has no nameserver, and the query fails
/// `TRY_AGAIN` at once (`resolvn_oracle.txt`). A failure sets `h_errno` and
/// `statp->res_h_errno`; an answer leaves both alone.
///
/// # Safety
///
/// `statp` is a `ResState` the caller owns, zeroed or from `res_ninit`; the
/// rest is as for [`res_query`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn res_nquery(
    statp: *mut ResState,
    dname: *const u8,
    class: i32,
    type_: i32,
    answer: *mut u8,
    anslen: i32,
) -> i32 {
    if statp.is_null() {
        errno::set_errno(errno::EFAULT);
        crate::socket::set_h_errno(NETDB_INTERNAL);
        return -1;
    }
    let Some((name, buf)) = args(dname, answer, anslen) else {
        // SAFETY: the caller's state.
        unsafe { set_herr(statp, NETDB_INTERNAL) };
        return -1;
    };
    // SAFETY: the caller's state, only read while the query runs.
    let r = query(unsafe { &*statp }, name, class, type_, buf);
    // SAFETY: as above; the query's borrow has ended.
    unsafe { finish(statp, r) }
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
) -> Result<usize, i32> {
    let mut full = [0u8; MAXDNAME];
    let Some(n) = join(name, domain, &mut full) else {
        return Err(crate::socket::NO_RECOVERY);
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
    // SAFETY: `_res`, initialised.
    unsafe { res_nquerydomain(global(), name, domain, class, type_, answer, anslen) }
}

/// `res_nquerydomain` -- [`res_querydomain`] with the caller's own resolver
/// state `statp`, used as it is (see [`res_nquery`]).
///
/// # Safety
///
/// As [`res_nquery`], with `domain` NULL or a NUL-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn res_nquerydomain(
    statp: *mut ResState,
    name: *const u8,
    domain: *const u8,
    class: i32,
    type_: i32,
    answer: *mut u8,
    anslen: i32,
) -> i32 {
    if statp.is_null() {
        errno::set_errno(errno::EFAULT);
        crate::socket::set_h_errno(NETDB_INTERNAL);
        return -1;
    }
    let Some((name, buf)) = args(name, answer, anslen) else {
        // SAFETY: the caller's state.
        unsafe { set_herr(statp, NETDB_INTERNAL) };
        return -1;
    };
    // SAFETY: the caller's contract: NULL or a NUL-terminated string.
    let domain = (!domain.is_null())
        .then(|| unsafe { core::slice::from_raw_parts(domain, crate::string::strlen(domain)) });
    // SAFETY: the caller's state, only read while the query runs.
    let r = querydomain(unsafe { &*statp }, name, domain, class, type_, buf);
    // SAFETY: as above; the query's borrow has ended.
    unsafe { finish(statp, r) }
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
) -> Result<usize, i32> {
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
    search_with(
        name,
        st.ndots(),
        st.options,
        domains.get(..nd).unwrap_or(&[]),
        |domain| {
            errno::set_errno(0);
            querydomain(st, name, domain, class, type_, answer).map_err(|h| {
                let servfail = answer.get(3).is_some_and(|b| b & 0xf == SERVFAIL);
                (h, errno::get_errno() == errno::ECONNREFUSED, servfail)
            })
        },
    )
}

/// `res_search` -- query for `name` through the search list, glibc's way:
/// as it stands first if it has `ndots` dots or ends in a dot, then with
/// each search domain (`RES_DNSRCH`; just the first with only
/// `RES_DEFNAMES`), then as it stands if not yet tried (unless
/// `RES_NOTLDQUERY` and it has no dots).  The first answer wins; otherwise
/// `h_errno` is the as-is query's, else `NO_DATA` if any domain had the
/// name, else `TRY_AGAIN` for a server failure.  `h_errno` is
/// `HOST_NOT_FOUND` from the start, as glibc's is, so an answer leaves it
/// so.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_search(
    dname: *const u8,
    class: i32,
    type_: i32,
    answer: *mut u8,
    anslen: i32,
) -> i32 {
    // SAFETY: `_res`, initialised.
    unsafe { res_nsearch(global(), dname, class, type_, answer, anslen) }
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

/// `res_nsearch` -- [`res_search`] with the caller's own resolver state
/// `statp`, used as it is (see [`res_nquery`]).
///
/// # Safety
///
/// As [`res_nquery`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn res_nsearch(
    statp: *mut ResState,
    dname: *const u8,
    class: i32,
    type_: i32,
    answer: *mut u8,
    anslen: i32,
) -> i32 {
    if statp.is_null() {
        errno::set_errno(errno::EFAULT);
        crate::socket::set_h_errno(NETDB_INTERNAL);
        return -1;
    }
    let Some((name, buf)) = args(dname, answer, anslen) else {
        // SAFETY: the caller's state.
        unsafe { set_herr(statp, NETDB_INTERNAL) };
        return -1;
    };
    // glibc's "true if we never query": HOST_NOT_FOUND before the first,
    // which a search that is answered leaves in place (`resolvn_oracle.txt`).
    // SAFETY: the caller's state.
    unsafe { set_herr(statp, crate::socket::HOST_NOT_FOUND) };
    // SAFETY: the caller's state, only read while the search runs.
    let r = search(unsafe { &*statp }, name, class, type_, buf);
    // SAFETY: as above; the search's borrow has ended.
    unsafe { finish(statp, r) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    std::thread_local! {
        /// Whether `send_query` asks [`Responder`] instead of the network.
        static RESPONDER_ON: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    }

    /// `send_query`'s hook: the exchange with [`Responder`] when a test has
    /// turned it on, else `None`, for the network.
    pub(super) fn scripted_exchange(
        sv: &Servers,
        q: &[u8],
        answer: &mut [u8],
    ) -> Option<Result<usize, i32>> {
        if !RESPONDER_ON.with(core::cell::Cell::get) {
            return None;
        }
        Some(exchange(&mut Responder::default(), sv, q, answer))
    }

    /// `resolvn_harness.py`'s responder, in-process: `a.example.test` has
    /// the A record 10.0.0.1, `srv.example.test` answers SERVFAIL, every
    /// other name NXDOMAIN.
    #[derive(Default)]
    struct Responder {
        pending: std::collections::VecDeque<Vec<u8>>,
        now: u64,
    }

    impl Transport for Responder {
        fn send_udp(&mut self, _i: usize, q: &[u8]) {
            self.pending.push_back(respond(q));
        }
        fn recv_udp(&mut self, buf: &mut [u8], ms: u64) -> Option<(usize, Option<usize>, bool)> {
            let Some(r) = self.pending.pop_front() else {
                self.now += ms;
                return None;
            };
            let keep = r.len().min(buf.len());
            buf[..keep].copy_from_slice(&r[..keep]);
            Some((r.len(), Some(0), false))
        }
        fn tcp(&mut self, _i: usize, _q: &[u8], _buf: &mut [u8], _ms: u64) -> Option<usize> {
            None
        }
        fn now_ms(&mut self) -> u64 {
            self.now
        }
    }

    fn respond(q: &[u8]) -> Vec<u8> {
        let mut at = 12;
        let mut name = Vec::new();
        while at < q.len() && q[at] != 0 {
            let l = usize::from(q[at]);
            at += 1;
            if !name.is_empty() {
                name.push(b'.');
            }
            name.extend_from_slice(&q[at..at + l]);
            at += l;
        }
        let qend = at + 1 + 4;
        let mut r = q[..qend].to_vec();
        r[2] = 0x80 | (q[2] & 1);
        r[3] = 0x80;
        r[4..12].copy_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
        let qtype = u16::from_be_bytes([q[at + 1], q[at + 2]]);
        if name.eq_ignore_ascii_case(b"a.example.test") && qtype == 1 {
            r.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 1, 0x2c, 0, 4, 10, 0, 0, 1]);
            r[7] = 1;
        } else if name.eq_ignore_ascii_case(b"srv.example.test") {
            r[3] |= 2;
        } else {
            r[3] |= 3;
        }
        r
    }

    /// An answer as the harness sums it up: `rcode ancount A`.
    fn summary(a: &[u8], rc: i32) -> String {
        if rc < 12 {
            return "- - -".into();
        }
        let rcode = a[3] & 0xf;
        let an = u16::from_be_bytes([a[6], a[7]]);
        let mut addr = "-".to_string();
        let mut h = core::mem::MaybeUninit::<NsMsg>::zeroed();
        // SAFETY: `a` holds `rc` bytes; the handle is written by the call.
        if an > 0 && unsafe { ns_initparse(a.as_ptr(), rc, h.as_mut_ptr()) } == 0 {
            // SAFETY: filled by ns_initparse.
            let mut h = unsafe { h.assume_init() };
            for k in 0..i32::from(an) {
                let mut rr = core::mem::MaybeUninit::<NsRr>::zeroed();
                // SAFETY: a live handle and a record to fill.
                if unsafe { ns_parserr(&raw mut h, 1, k, rr.as_mut_ptr()) } == 0 {
                    // SAFETY: filled by ns_parserr.
                    let rr = unsafe { rr.assume_init() };
                    if rr.type_ == 1 && rr.rdlength == 4 {
                        // SAFETY: four bytes of rdata inside the answer.
                        let d = unsafe { core::slice::from_raw_parts(rr.rdata, 4) };
                        addr = format!("{}.{}.{}.{}", d[0], d[1], d[2], d[3]);
                        break;
                    }
                }
            }
        }
        format!("{rcode} {an} {addr}")
    }

    /// glibc 2.39's reentrant resolver (`resolvn_harness.py`), replayed: the
    /// state from the harness's `resolv.conf`, the responder in place of the
    /// network, `h_errno` and `res_h_errno` set to 12345 before each call.
    #[test]
    fn the_reentrant_resolver_answers_as_glibcs() {
        const ORACLE: &str = include_str!("resolvn_oracle.txt");
        let conf: Vec<u8> = ORACLE
            .lines()
            .filter_map(|l| l.strip_prefix("#   "))
            .flat_map(|l| l.bytes().chain(Some(b'\n')))
            .collect();
        // What res_ninit makes of that file, built where it stays: the state
        // points into itself (`dnsrch` into `defdname`), as glibc's does, so
        // a moved copy's search list would point at the old one.
        let mut st = ResState::ZERO;
        store_conf(&mut st, &parse_conf(&conf, b""));
        RESPONDER_ON.with(|r| r.set(true));
        let mut qb = [0u8; 512];
        let mut qlen = 0;
        let mut zero = ResState::ZERO;
        for line in ORACLE.lines().filter(|l| !l.starts_with('#')) {
            let (case, glibc) = line.split_once(" = ").expect("<case> = <answer>");
            let ask = |f: &mut dyn FnMut(*mut ResState, &mut [u8]) -> i32, sp: *mut ResState| {
                let mut buf = [0u8; 1024];
                crate::socket::set_h_errno(12345);
                // SAFETY: a live state.
                unsafe { (*sp).res_h_errno = 12345 };
                let rc = f(sp, &mut buf);
                // SAFETY: as above.
                let res_h = unsafe { (*sp).res_h_errno };
                let sum = if rc < 0 {
                    "- - -".to_string()
                } else {
                    summary(&buf, rc)
                };
                format!(
                    "{} {} {res_h} {sum}",
                    if rc < 0 { -1 } else { 1 },
                    crate::socket::get_h_errno()
                )
            };
            let q = |name: &str| {
                let mut n = name.as_bytes().to_vec();
                n.push(0);
                n
            };
            let ours = match case {
                "I zeroed" => {
                    let s0 = &st.nsaddr_list[0];
                    let a = s0.sin_addr.s_addr.to_ne_bytes();
                    let mut line = format!(
                        "0 {} {} {:x} {} {}.{}.{}.{}:{} {}",
                        st.retrans,
                        st.retry,
                        st.options,
                        st.nscount,
                        a[0],
                        a[1],
                        a[2],
                        a[3],
                        u16::from_be(s0.sin_port),
                        st.ndots()
                    );
                    for p in st.dnsrch.iter().take_while(|p| !p.is_null()) {
                        // SAFETY: into `defdname`, NUL-terminated.
                        let d =
                            unsafe { core::slice::from_raw_parts(*p, crate::string::strlen(*p)) };
                        line.push(' ');
                        line.push_str(core::str::from_utf8(d).unwrap());
                    }
                    line.push_str(" ;");
                    line
                }
                "R again" => {
                    // A state of its own: `st` must keep the harness's file.
                    let mut again = ResState::ZERO;
                    // SAFETY: a live state, initialised and then again.
                    unsafe { __res_ninit(&raw mut again) };
                    // SAFETY: as above.
                    unsafe { __res_ninit(&raw mut again) }.to_string()
                }
                c if c.starts_with("Q ") => {
                    let (name, t) = match &c[2..] {
                        "a.example.test/AAAA" => ("a.example.test", T_AAAA),
                        n => (n, T_A),
                    };
                    let n = q(name);
                    ask(
                        &mut |sp, b: &mut [u8]| unsafe {
                            res_nquery(sp, n.as_ptr(), C_IN, t, b.as_mut_ptr(), 1024)
                        },
                        &raw mut st,
                    )
                }
                c if c.starts_with("S ") => {
                    let n = q(&c[2..]);
                    ask(
                        &mut |sp, b: &mut [u8]| unsafe {
                            res_nsearch(sp, n.as_ptr(), C_IN, T_A, b.as_mut_ptr(), 1024)
                        },
                        &raw mut st,
                    )
                }
                c if c.starts_with("D ") => {
                    let (name, dom) = c[2..].split_once(' ').unwrap();
                    let (n, d) = (q(name), q(dom));
                    ask(
                        &mut |sp, b: &mut [u8]| unsafe {
                            res_nquerydomain(
                                sp,
                                n.as_ptr(),
                                d.as_ptr(),
                                C_IN,
                                T_A,
                                b.as_mut_ptr(),
                                1024,
                            )
                        },
                        &raw mut st,
                    )
                }
                "M a.example.test" => {
                    let n = q("a.example.test");
                    // SAFETY: a live state and buffer.
                    let rc = unsafe {
                        res_nmkquery(
                            &raw mut st,
                            QUERY,
                            n.as_ptr(),
                            C_IN,
                            T_A,
                            core::ptr::null(),
                            0,
                            core::ptr::null(),
                            qb.as_mut_ptr(),
                            512,
                        )
                    };
                    qlen = rc;
                    let bytes: String = qb[..usize::try_from(rc).unwrap()]
                        .iter()
                        .enumerate()
                        .map(|(k, b)| {
                            if k < 2 {
                                "00".to_string()
                            } else {
                                format!("{b:02x}")
                            }
                        })
                        .collect();
                    format!("{rc} {bytes}")
                }
                "N" => {
                    let mut ab = [0u8; 1024];
                    // SAFETY: a live state and buffers.
                    let n =
                        unsafe { res_nsend(&raw mut st, qb.as_ptr(), qlen, ab.as_mut_ptr(), 1024) };
                    format!(
                        "{} {}",
                        if n < 0 { -1 } else { 1 },
                        if n < 0 {
                            "- - -".into()
                        } else {
                            summary(&ab, n)
                        }
                    )
                }
                c if c.starts_with("U ") => {
                    let n = q("a.example.test");
                    let one = q("a");
                    let dom = q("example.test");
                    let mut buf = [0u8; 1024];
                    crate::socket::set_h_errno(12345);
                    zero.res_h_errno = 12345;
                    // SAFETY: a live (zeroed) state and buffers.
                    let rc = unsafe {
                        match &c[2..] {
                            "query" => res_nquery(
                                &raw mut zero,
                                n.as_ptr(),
                                C_IN,
                                T_A,
                                buf.as_mut_ptr(),
                                1024,
                            ),
                            "search" => res_nsearch(
                                &raw mut zero,
                                one.as_ptr(),
                                C_IN,
                                T_A,
                                buf.as_mut_ptr(),
                                1024,
                            ),
                            "querydomain" => res_nquerydomain(
                                &raw mut zero,
                                one.as_ptr(),
                                dom.as_ptr(),
                                C_IN,
                                T_A,
                                buf.as_mut_ptr(),
                                1024,
                            ),
                            "mkquery" => res_nmkquery(
                                &raw mut zero,
                                QUERY,
                                n.as_ptr(),
                                C_IN,
                                T_A,
                                core::ptr::null(),
                                0,
                                core::ptr::null(),
                                buf.as_mut_ptr(),
                                1024,
                            ),
                            "send" => {
                                res_nsend(&raw mut zero, qb.as_ptr(), 32, buf.as_mut_ptr(), 1024)
                            }
                            other => {
                                panic!("an uninitialised call this test does not know: {other}")
                            }
                        }
                    };
                    format!("{rc} {} {}", crate::socket::get_h_errno(), zero.res_h_errno)
                }
                "C init-bit" => {
                    // SAFETY: a live state.
                    unsafe { __res_nclose(&raw mut st) };
                    i32::from(st.options & RES_INIT != 0).to_string()
                }
                other => panic!("a case this test does not know: {other}"),
            };
            assert_eq!(ours, glibc, "{case}");
        }
        RESPONDER_ON.with(|r| r.set(false));
    }

    /// A line's text token back into bytes: `\xHH` for a byte, `\x` alone
    /// for the empty string (`nsname_harness.py`'s `token`).
    fn untoken(t: &str) -> Vec<u8> {
        if t == "\\x" {
            return Vec::new();
        }
        let b = t.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') {
                out.push(u8::from_str_radix(&t[i + 2..i + 4], 16).expect("hex"));
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    fn unhex(h: &str) -> Vec<u8> {
        if h == "-" {
            return Vec::new();
        }
        (0..h.len())
            .step_by(2)
            .map(|k| u8::from_str_radix(&h[k..k + 2], 16).expect("hex"))
            .collect()
    }

    fn hex(b: &[u8]) -> String {
        use core::fmt::Write;
        if b.is_empty() {
            return "-".into();
        }
        b.iter().fold(String::new(), |mut s, x| {
            // Writing into a String cannot fail.
            let _ = write!(s, "{x:02x}");
            s
        })
    }

    /// Text as the harness writes it.
    fn token(b: &[u8]) -> String {
        use core::fmt::Write;
        if b.is_empty() {
            return "\\x".into();
        }
        b.iter().fold(String::new(), |mut s, &c| {
            if (0x21..=0x7e).contains(&c) && c != b'\\' {
                s.push(c as char);
            } else {
                // Writing into a String cannot fail.
                let _ = write!(s, "\\x{c:02x}");
            }
            s
        })
    }

    /// The uncompressed wire name at the start of `b`: its bytes, up to the
    /// root label or the end.
    fn wire_prefix(b: &[u8]) -> &[u8] {
        let mut i = 0;
        while let Some(&n) = b.get(i) {
            if n == 0 {
                return &b[..=i];
            }
            i += 1 + usize::from(n);
        }
        &b[..b.len().min(i)]
    }

    /// glibc 2.39's `ns_name_*` and `res_*ok` (`nsname_harness.py`),
    /// replayed line by line; `errno` compared where the call failed.
    #[test]
    fn ns_name_and_the_name_checks_answer_as_glibcs() {
        use std::collections::HashMap;
        const ORACLE: &str = include_str!("nsname_oracle.txt");
        let mut msgs: HashMap<String, Vec<u8>> = HashMap::new();
        for line in include_str!("ns_oracle.txt").lines() {
            if let Some(rest) = line.strip_prefix("M ") {
                let (n, h) = rest.split_once(' ').expect("M <name> <hex>");
                msgs.insert(n.to_string(), unhex(h));
            }
        }
        let err = |rc: i32| -> i32 {
            if rc < 0 {
                crate::errno::get_errno()
            } else {
                1234
            }
        };
        // One pack sequence at a time: its message and table.
        let mut seq_name = String::new();
        let mut msg = vec![0u8; 1024];
        let mut table: Vec<*const u8> = Vec::new();
        let mut at = 12usize;
        let mut counted = HashMap::<char, usize>::new();
        for line in ORACLE.lines().filter(|l| !l.starts_with('#')) {
            let (call, glibc) = line.split_once(" = ").expect("<call> = <answer>");
            let mut f = call.split(' ');
            let kind = f.next().expect("a kind").chars().next().expect("a letter");
            *counted.entry(kind).or_default() += 1;
            crate::errno::set_errno(1234);
            let ours = match kind {
                'T' => {
                    let text = untoken(f.next().unwrap());
                    let n: usize = f.next().unwrap().parse().unwrap();
                    let mut src = text.clone();
                    src.push(0);
                    let mut dst = vec![0xeeu8; 2048];
                    // SAFETY: a NUL-terminated text; `dst` holds `n` bytes.
                    let rc = unsafe { ns_name_pton(src.as_ptr(), dst.as_mut_ptr(), n) };
                    let wire = if rc < 0 {
                        &[][..]
                    } else {
                        wire_prefix(&dst[..n])
                    };
                    format!("{rc} {} {}", err(rc), hex(wire))
                }
                'N' => {
                    let src = unhex(f.next().unwrap());
                    let n: usize = f.next().unwrap().parse().unwrap();
                    let mut dst = vec![0u8; 2048];
                    // SAFETY: the wire name ends in the vector; `dst` holds `n`.
                    let rc = unsafe { ns_name_ntop(src.as_ptr(), dst.as_mut_ptr(), n) };
                    let text = if rc < 0 {
                        "-".to_string()
                    } else {
                        let end = dst.iter().position(|&c| c == 0).unwrap();
                        token(&dst[..end])
                    };
                    format!("{rc} {} {text}", err(rc))
                }
                'U' => {
                    let m = &msgs[f.next().unwrap()];
                    let o: usize = f.next().unwrap().parse().unwrap();
                    let n: usize = f.next().unwrap().parse().unwrap();
                    let mut dst = vec![0xeeu8; 2048];
                    // SAFETY: the message and `dst`, as the contract asks.
                    let rc = unsafe {
                        ns_name_unpack(
                            m.as_ptr(),
                            m.as_ptr().add(m.len()),
                            m.as_ptr().add(o),
                            dst.as_mut_ptr(),
                            n,
                        )
                    };
                    let wire = if rc < 0 {
                        &[][..]
                    } else {
                        wire_prefix(&dst[..n])
                    };
                    format!("{rc} {} {}", err(rc), hex(wire))
                }
                'S' => {
                    let m = &msgs[f.next().unwrap()];
                    let o: usize = f.next().unwrap().parse().unwrap();
                    let e: usize = f.next().unwrap().parse().unwrap();
                    let mut p = m.as_ptr().wrapping_add(o);
                    // SAFETY: `[p, m + e)` is inside the message.
                    let rc = unsafe { ns_name_skip(&raw mut p, m.as_ptr().add(e)) };
                    format!("{rc} {} {}", err(rc), p as usize - m.as_ptr() as usize)
                }
                'P' => {
                    let seq = f.next().unwrap();
                    let _i = f.next().unwrap();
                    let how = f.next().unwrap();
                    let name = f.next().unwrap();
                    let n: usize = f.next().unwrap().parse().unwrap();
                    if seq != seq_name {
                        seq_name = seq.to_string();
                        msg = vec![0u8; 1024];
                        at = 12;
                        let room = if seq == "full" { 2 } else { 8 };
                        table = vec![core::ptr::null(); room + 2];
                        table[0] = msg.as_ptr();
                    }
                    let nocomp = seq == "nocomp";
                    let (dnptrs, last) = if nocomp {
                        (core::ptr::null_mut(), core::ptr::null_mut())
                    } else {
                        let len = table.len();
                        (table.as_mut_ptr(), table.as_mut_ptr().wrapping_add(len - 1))
                    };
                    let dst = msg.as_mut_ptr().wrapping_add(at);
                    // SAFETY: `dst` is inside `msg`, which the table's names are in.
                    let rc = unsafe {
                        if how == "wire" {
                            let src = unhex(name);
                            ns_name_pack(src.as_ptr(), dst, i32::try_from(n).unwrap(), dnptrs, last)
                        } else {
                            let mut src = untoken(name);
                            src.push(0);
                            ns_name_compress(src.as_ptr(), dst, n, dnptrs, last)
                        }
                    };
                    let written = if rc > 0 {
                        hex(&msg[at..at + rc as usize])
                    } else {
                        "-".into()
                    };
                    let mut tab = String::new();
                    if nocomp {
                        tab.push_str(" -");
                    } else {
                        for &q in table.iter().take_while(|q| !q.is_null()) {
                            tab.push_str(&format!(" {}", q as usize - msg.as_ptr() as usize));
                        }
                        tab.push_str(" -");
                    }
                    if rc > 0 {
                        at += rc as usize;
                    }
                    // glibc leaves `errno` at dn_find's ENOENT after a
                    // successful pack; only a failure's is compared.
                    let e = if rc < 0 {
                        err(rc).to_string()
                    } else {
                        glibc.split(' ').nth(1).unwrap().to_string()
                    };
                    format!("{rc} {e} {written} ;{tab}")
                }
                'O' => {
                    let mut dn = untoken(f.next().unwrap());
                    dn.push(0);
                    let p = dn.as_ptr();
                    format!(
                        "{} {} {} {}",
                        res_hnok(p),
                        res_ownok(p),
                        res_mailok(p),
                        res_dnok(p)
                    )
                }
                other => panic!("a kind this test does not know: {other}"),
            };
            assert_eq!(ours, glibc, "{call}");
        }
        assert!(counted.values().sum::<usize>() > 300, "{counted:?}");
    }

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
        assert_eq!(st.options, FRESH_OPTIONS | RES_INIT);
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
        assert_eq!(st.options & FRESH_OPTIONS, FRESH_OPTIONS);
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
        // The header's wider types: only the field's own bits are written.
        ns_put16(0x1_2345, b.as_mut_ptr());
        assert_eq!(&b[..2], &[0x23, 0x45]);
        ns_put32(0x1_0000_0001, b.as_mut_ptr());
        assert_eq!(b, [0, 0, 0, 1]);
    }

    /// A pointer back to a name that ends before the pointer: the length is
    /// the pointer's two bytes. `name_unpack` used to subtract its position
    /// from the start regardless, which overflowed -- a panic in a debug
    /// build -- on every such name, the common shape of a DNS answer.
    #[test]
    fn a_pointer_back_to_an_earlier_name_is_two_bytes() {
        let mut m = std::vec![0u8; 12];
        m.extend_from_slice(b"\x01a\x00"); // 12..15: "a"
        m.extend_from_slice(b"\xff\xff"); // filler
        m.extend_from_slice(&[0xc0, 12]); // 17: a pointer to 12
        let mut out = [0u8; 64];
        let base = m.as_ptr();
        // SAFETY: `m` is the message; `out` holds 64 bytes.
        let n = dn_expand(
            base,
            base.wrapping_add(m.len()),
            base.wrapping_add(17),
            out.as_mut_ptr(),
            64,
        );
        assert_eq!(n, 2);
        assert_eq!(&out[..2], b"a\0");
    }

    // -- ns_initparse / ns_parserr / ns_skiprr / ns_name_uncompress --

    /// glibc's answers (`posix/tools/oracle/ns_harness.py`): the messages,
    /// then one call a line; the harness's docstring has the format.
    const NS_ORACLE: &str = include_str!("ns_oracle.txt");

    /// A name as the harness prints one: `\xHH` outside `!`..`~` and for
    /// `\`, and `\x` alone for the empty name.
    fn ns_text(b: &[u8]) -> String {
        if b.is_empty() {
            return "\\x".into();
        }
        let mut s = String::new();
        for &c in b {
            if !(0x21..=0x7e).contains(&c) || c == b'\\' {
                s.push_str(&format!("\\x{c:02x}"));
            } else {
                s.push(char::from(c));
            }
        }
        s
    }

    /// A pointer as an offset into `m`, `-` for NULL, `?` outside it.
    fn ns_off(p: *const u8, m: &[u8]) -> String {
        let base = m.as_ptr() as usize;
        if p.is_null() {
            "-".into()
        } else if (p as usize) >= base && (p as usize) <= base + m.len() {
            (p as usize - base).to_string()
        } else {
            "?".into()
        }
    }

    fn ns_handle(h: &NsMsg, m: &[u8]) -> String {
        let mut s = format!("{} {}", h.id, h.flags);
        for c in h.counts {
            s.push_str(&format!(" {c}"));
        }
        for p in h.sections {
            s.push_str(&format!(" {}", ns_off(p, m)));
        }
        s.push_str(&format!(" {} {} {}", h.sect, h.rrnum, ns_off(h.msg_ptr, m)));
        s
    }

    fn empty_ns_msg() -> NsMsg {
        NsMsg {
            msg: core::ptr::null(),
            eom: core::ptr::null(),
            id: 0,
            flags: 0,
            counts: [0; 4],
            sections: [core::ptr::null(); 4],
            sect: 0,
            rrnum: 0,
            msg_ptr: core::ptr::null(),
        }
    }

    #[test]
    fn ns_parsing_answers_as_glibc_does() {
        let mut msgs: std::collections::BTreeMap<String, Vec<u8>> = Default::default();
        let mut handles: std::collections::BTreeMap<String, NsMsg> = Default::default();
        let mut bad = Vec::new();
        let mut calls = 0;
        for line in NS_ORACLE.lines().filter(|l| !l.starts_with('#')) {
            if let Some(rest) = line.strip_prefix("M ") {
                let (name, hex) = rest.split_once(' ').unwrap();
                let bytes = if hex == "-" {
                    Vec::new()
                } else {
                    (0..hex.len())
                        .step_by(2)
                        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                        .collect()
                };
                msgs.insert(name.into(), bytes);
                continue;
            }
            let (lhs, want) = line.split_once(" = ").unwrap();
            let w: Vec<&str> = lhs.split(' ').collect();
            let m = &msgs[w[1]];
            errno::set_errno(1234);
            let got = match w[0] {
                "I" => {
                    let mut h = empty_ns_msg();
                    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                    // SAFETY: the message and a handle of this frame's.
                    let rc = unsafe { ns_initparse(m.as_ptr(), m.len() as i32, &raw mut h) };
                    let err = errno::get_errno();
                    let mut s = format!("{rc} {err}");
                    if rc == 0 {
                        s.push(' ');
                        s.push_str(&ns_handle(&h, m));
                        handles.insert(w[1].into(), h);
                    }
                    s
                }
                "P" => {
                    let (sect, rrnum) = (w[2].parse().unwrap(), w[3].parse().unwrap());
                    let h = handles.get_mut(w[1]).unwrap();
                    // SAFETY: an all-zero record is a valid one: bytes, and
                    // a NULL pointer.
                    let mut rr: NsRr = unsafe { core::mem::zeroed() };
                    // SAFETY: a handle `ns_initparse` filled over a message
                    // still in `msgs`; a record of this frame's.
                    let rc = unsafe { ns_parserr(h, sect, rrnum, &raw mut rr) };
                    let err = errno::get_errno();
                    let mut s = format!("{rc} {err}");
                    if rc == 0 {
                        let n = rr.name.iter().position(|&b| b == 0).unwrap();
                        s.push_str(&format!(
                            " {} {} {} {} {} {}",
                            ns_text(&rr.name[..n]),
                            rr.type_,
                            rr.rr_class,
                            rr.ttl,
                            rr.rdlength,
                            ns_off(rr.rdata, m)
                        ));
                    }
                    s.push_str(" ; ");
                    s.push_str(&ns_handle(h, m));
                    s
                }
                "K" => {
                    let (off, sect, count, eom): (usize, i32, i32, usize) = (
                        w[2].parse().unwrap(),
                        w[3].parse().unwrap(),
                        w[4].parse().unwrap(),
                        w[5].parse().unwrap(),
                    );
                    let rc = ns_skiprr(
                        m.as_ptr().wrapping_add(off),
                        m.as_ptr().wrapping_add(eom),
                        sect,
                        count,
                    );
                    format!("{rc} {}", errno::get_errno())
                }
                "U" => {
                    let (off, size): (usize, usize) =
                        (w[2].parse().unwrap(), w[3].parse().unwrap());
                    let mut out = [0u8; 1100];
                    // SAFETY: the message; `out` holds more than `size`.
                    let rc = unsafe {
                        ns_name_uncompress(
                            m.as_ptr(),
                            m.as_ptr().wrapping_add(m.len()),
                            m.as_ptr().wrapping_add(off),
                            out.as_mut_ptr(),
                            size,
                        )
                    };
                    let mut s = format!("{rc} {}", errno::get_errno());
                    if rc >= 0 {
                        let n = out.iter().position(|&b| b == 0).unwrap();
                        s.push(' ');
                        s.push_str(&ns_text(&out[..n]));
                    }
                    s
                }
                other => panic!("no such line: {other}"),
            };
            calls += 1;
            if got != want {
                bad.push(format!("{line}\n    ours {got}"));
            }
        }
        assert!(calls > 120, "only {calls} calls");
        assert!(
            bad.is_empty(),
            "{} of {calls} differ:\n{}",
            bad.len(),
            bad.join("\n")
        );
    }

    #[test]
    fn ns_calls_refuse_what_glibc_would_fault_on() {
        let m = [0u8; 12];
        // SAFETY: NULLs are what is tested.
        unsafe {
            assert_eq!(ns_initparse(m.as_ptr(), 12, core::ptr::null_mut()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
            let mut h = empty_ns_msg();
            assert_eq!(ns_initparse(core::ptr::null(), 12, &raw mut h), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
            // Too short to hold a header: EMSGSIZE, before `msg` is read.
            assert_eq!(ns_initparse(core::ptr::null(), 0, &raw mut h), -1);
            assert_eq!(errno::get_errno(), errno::EMSGSIZE);
            assert_eq!(ns_initparse(m.as_ptr(), -5, &raw mut h), -1);
            assert_eq!(errno::get_errno(), errno::EMSGSIZE);
            assert_eq!(ns_initparse(m.as_ptr(), 12, &raw mut h), 0);
            assert_eq!(ns_parserr(&raw mut h, 0, 0, core::ptr::null_mut()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }
    }

    #[test]
    fn ns_flagdata_is_the_headers_flag_table() {
        // qr, opcode, aa, tc, rd, ra, z, ad, cd, rcode of a response's 0x8583.
        let flags = 0x8583;
        let get = |f: usize| (flags & _ns_flagdata[f].mask) >> _ns_flagdata[f].shift;
        assert_eq!(
            (0..10).map(get).collect::<Vec<_>>(),
            [1, 0, 1, 0, 1, 1, 0, 0, 0, 3]
        );
    }
}
