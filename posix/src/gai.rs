// Indices and sums here are over a fixed table (the socket types), a
// sixteen-byte address, or a result list whose length the loop bounds.
// Clippy cannot see the bounds.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
//! `getaddrinfo`, `freeaddrinfo`, `gai_strerror` and `getnameinfo`: glibc
//! 2.40's (`nss/getaddrinfo.c`, `nss/getnameinfo.c`), over this library's
//! hosts database ([`crate::hosts`]).
//!
//! What a caller sees, step by step, as glibc does it:
//!
//! - **The arguments.**  Unknown `ai_flags` are `EAI_BADFLAGS`, as is
//!   `AI_CANONNAME` without a name; a family other than `AF_UNSPEC`,
//!   `AF_INET` and `AF_INET6` is `EAI_FAMILY`.  A name or service of `*` is
//!   no name or service at all.
//! - **`AI_ADDRCONFIG`** asks what addresses this machine has: with IPv4
//!   and no IPv6 (loopback addresses not counting), an `AF_UNSPEC` question
//!   becomes an IPv4 one.
//! - **The service** is a decimal number (`strtoul`: `" 80"` and `"+80"`
//!   are 80; the number is not checked against 65535, as glibc does not
//!   check it) or a name from the services database; each socket type the
//!   hints allow (`SOCK_STREAM`/TCP, `SOCK_DGRAM`/UDP, `SOCK_RAW` when there
//!   is no service -- and DCCP, UDP-Lite and SCTP when asked for by name)
//!   gets an entry.
//! - **The host** is, in order: none (the loopback, or with `AI_PASSIVE`
//!   the wildcard -- IPv6 first, as glibc lists them); a number
//!   (`inet_aton`'s forms for IPv4, `inet_pton`'s for IPv6, with a `%scope`
//!   suffix); or a name, looked up in the hosts database the way
//!   `nsswitch.conf`'s `files dns` would.  `AI_V4MAPPED` and `AI_ALL` map
//!   IPv4 answers into an IPv6 question.
//! - **The answer** is one `struct addrinfo` per address and socket type,
//!   each its own `malloc` block with its address in it; the canonical
//!   name, when asked for, is a separate block on the first entry.
//!   `freeaddrinfo` frees both, as glibc's does.
//! - **More than one address** is sorted by RFC 3484's destination rules
//!   with glibc's tables, or `/etc/gai.conf`'s.  The first rule --
//!   "avoid unusable destinations" -- is decided as glibc decides it, by
//!   connecting a datagram socket: on this system an IPv6 destination has
//!   no socket to connect, so IPv6 answers sort after IPv4 ones.
//!
//! Where this differs from glibc, the kernel's resolver is why: it answers
//! one IPv4 address and no canonical name (see [`crate::hosts`]).  An
//! internationalized name (`AI_IDN`) has no `libidn2` to encode it, so it
//! is `EAI_IDN_ENCODE`, as glibc answers without the library.

use crate::errno;
use crate::hosts::{self, NETDB_INTERNAL, Status, Tuple};
use crate::socket::{
    AF_INET, AF_INET6, AF_UNSPEC, Addrinfo, EAI_ADDRFAMILY, EAI_AGAIN, EAI_BADFLAGS, EAI_FAMILY,
    EAI_MEMORY, EAI_NODATA, EAI_NONAME, EAI_SERVICE, EAI_SOCKTYPE, EAI_SYSTEM, IPPROTO_TCP,
    IPPROTO_UDP, In6Addr, InAddr, NO_DATA, SOCK_DGRAM, SOCK_RAW, SOCK_STREAM, Sockaddr, SockaddrIn,
    SockaddrIn6, SocklenT, TRY_AGAIN,
};

// ---------------------------------------------------------------------------
// Flags and codes
// ---------------------------------------------------------------------------

pub use crate::socket::{AI_CANONNAME, AI_NUMERICHOST, AI_NUMERICSERV, AI_PASSIVE};
/// Map IPv4 answers into an IPv6 question.
pub const AI_V4MAPPED: i32 = 0x0008;
/// With `AI_V4MAPPED`: the mapped IPv4 answers as well as the IPv6 ones.
pub const AI_ALL: i32 = 0x0010;
/// Answer only in the families this machine has addresses in.
pub const AI_ADDRCONFIG: i32 = 0x0020;
/// Encode an internationalized name (glibc's; musl's header lacks it).
pub const AI_IDN: i32 = 0x0040;
/// Decode the canonical name (glibc's).
pub const AI_CANONIDN: i32 = 0x0080;
/// `AI_IDN_ALLOW_UNASSIGNED` and `AI_IDN_USE_STD3_ASCII_RULES`: accepted
/// and ignored, as glibc accepts them.
const DEPRECATED_AI_IDN: i32 = 0x0300;

/// An internationalized name that could not be encoded.
pub const EAI_IDN_ENCODE: i32 = -105;

/// `SOCK_DCCP`.
const SOCK_DCCP: i32 = 6;
/// `SOCK_SEQPACKET`.
const SOCK_SEQPACKET: i32 = 5;
const IPPROTO_DCCP: i32 = 33;
const IPPROTO_UDPLITE: i32 = 136;
const IPPROTO_SCTP: i32 = 132;

/// `%`: what ends an IPv6 address and begins its scope.
const SCOPE_DELIMITER: u8 = b'%';

// ---------------------------------------------------------------------------
// A small growable list, for the addresses a lookup finds
// ---------------------------------------------------------------------------

/// A `malloc`-backed list of `Copy` values.
struct List<T: Copy> {
    ptr: *mut T,
    len: usize,
    cap: usize,
    /// A `realloc` failed: the list is no longer complete.
    failed: bool,
}

impl<T: Copy> List<T> {
    const fn new() -> Self {
        Self {
            ptr: core::ptr::null_mut(),
            len: 0,
            cap: 0,
            failed: false,
        }
    }

    fn push(&mut self, v: T) {
        if self.failed {
            return;
        }
        if self.len == self.cap {
            let cap = if self.cap == 0 { 4 } else { self.cap * 2 };
            let Some(bytes) = cap.checked_mul(size_of::<T>()) else {
                self.failed = true;
                return;
            };
            // SAFETY: `ptr` is NULL or this list's block.
            let p = unsafe { crate::malloc::realloc(self.ptr.cast(), bytes) }.cast::<T>();
            if p.is_null() {
                self.failed = true;
                return;
            }
            self.ptr = p;
            self.cap = cap;
        }
        // SAFETY: `len < cap`, inside the block.
        unsafe { self.ptr.add(self.len).write(v) };
        self.len += 1;
    }

    fn as_slice(&self) -> &[T] {
        if self.ptr.is_null() {
            return &[];
        }
        // SAFETY: `len` values are written.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        if self.ptr.is_null() {
            return &mut [];
        }
        // SAFETY: as above.
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    fn clear(&mut self) {
        self.len = 0;
        self.failed = false;
    }
}

impl<T: Copy> Drop for List<T> {
    fn drop(&mut self) {
        // SAFETY: NULL or this list's block.
        unsafe { crate::malloc::free(self.ptr.cast()) };
    }
}

/// A `malloc`ed copy of `s` with a NUL: glibc's `strdup`.  NULL when memory
/// runs out.
fn strdup(s: &[u8]) -> *mut u8 {
    let p = crate::malloc::malloc(s.len() + 1);
    if !p.is_null() {
        // SAFETY: a fresh block of `len + 1` bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
            p.add(s.len()).write(0);
        }
    }
    p
}

// ---------------------------------------------------------------------------
// Services and socket types
// ---------------------------------------------------------------------------

/// One socket type `getaddrinfo` knows: glibc's `gaih_typeproto`.
struct TypeProto {
    socktype: i32,
    protocol: i32,
    /// `GAI_PROTO_NOSERVICE`: no service can be named for it.
    noservice: bool,
    /// `GAI_PROTO_PROTOANY`: the hints' protocol is its protocol.
    protoany: bool,
    /// Listed when the hints name no type and no protocol.
    default: bool,
    /// Its name in the services database.
    name: &'static [u8],
}

/// glibc's `gaih_inet_typeproto`, less the sentinels.
const TYPEPROTO: [TypeProto; 7] = [
    TypeProto {
        socktype: SOCK_STREAM,
        protocol: IPPROTO_TCP,
        noservice: false,
        protoany: false,
        default: true,
        name: b"tcp\0",
    },
    TypeProto {
        socktype: SOCK_DGRAM,
        protocol: IPPROTO_UDP,
        noservice: false,
        protoany: false,
        default: true,
        name: b"udp\0",
    },
    TypeProto {
        socktype: SOCK_DCCP,
        protocol: IPPROTO_DCCP,
        noservice: false,
        protoany: false,
        default: false,
        name: b"dccp\0",
    },
    TypeProto {
        socktype: SOCK_DGRAM,
        protocol: IPPROTO_UDPLITE,
        noservice: false,
        protoany: false,
        default: false,
        name: b"udplite\0",
    },
    TypeProto {
        socktype: SOCK_STREAM,
        protocol: IPPROTO_SCTP,
        noservice: false,
        protoany: false,
        default: false,
        name: b"sctp\0",
    },
    TypeProto {
        socktype: SOCK_SEQPACKET,
        protocol: IPPROTO_SCTP,
        noservice: false,
        protoany: false,
        default: false,
        name: b"sctp\0",
    },
    TypeProto {
        socktype: SOCK_RAW,
        protocol: 0,
        noservice: true,
        protoany: true,
        default: true,
        name: b"raw\0",
    },
];

/// One socket type and port an answer is given for: glibc's
/// `gaih_servtuple`.
#[derive(Clone, Copy, Default)]
struct ServTuple {
    socktype: i32,
    protocol: i32,
    /// Network byte order.
    port: u16,
}

/// The service a question names: its NUL-terminated text, and its number
/// when it is one (`-1` otherwise) -- glibc's `gaih_service`.
struct Service {
    name: *const u8,
    num: i32,
}

/// The hints `getaddrinfo` works from.
#[derive(Clone, Copy)]
struct Req {
    flags: i32,
    family: i32,
    socktype: i32,
    protocol: i32,
}

/// `gaih_inet_serv`: the port the services database gives `service` for
/// `tp`'s protocol, `EAI_SERVICE` when it has none.
fn serv_tuple(service: &Service, tp: &TypeProto, req: &Req) -> Result<ServTuple, i32> {
    let mut s = crate::netdb::Servent {
        s_name: core::ptr::null(),
        s_aliases: core::ptr::null(),
        s_port: 0,
        s_proto: core::ptr::null(),
    };
    let mut result: *const crate::netdb::Servent = core::ptr::null();
    let mut len = 1024usize;
    loop {
        let buf = crate::malloc::malloc(len);
        if buf.is_null() {
            return Err(EAI_MEMORY);
        }
        // SAFETY: `name` is the caller's NUL-terminated service; the outputs
        // and `len` bytes at `buf` are this function's.
        let r = unsafe {
            crate::netdb::getservbyname_r(
                service.name,
                tp.name.as_ptr(),
                &mut s,
                buf,
                len,
                &mut result,
            )
        };
        let port = (!result.is_null()).then_some(s.s_port);
        // SAFETY: `buf` is this function's block, and `port` is copied out.
        unsafe { crate::malloc::free(buf) };
        if r == errno::ERANGE {
            len *= 2;
            continue;
        }
        let Some(port) = port.filter(|_| r == 0) else {
            return Err(EAI_SERVICE);
        };
        return Ok(ServTuple {
            socktype: tp.socktype,
            protocol: if tp.protoany {
                req.protocol
            } else {
                tp.protocol
            },
            port: port as u16,
        });
    }
}

/// `get_servtuples`: the socket types and ports the answer is given for.
fn servtuples(service: Option<&Service>, req: &Req, out: &mut List<ServTuple>) -> Result<(), i32> {
    // The hints narrow the table to one entry, when they say anything.
    let mut chosen: Option<&TypeProto> = None;
    if req.protocol != 0 || req.socktype != 0 {
        chosen = TYPEPROTO.iter().find(|tp| {
            !((req.socktype != 0 && req.socktype != tp.socktype)
                || (req.protocol != 0 && !tp.protoany && req.protocol != tp.protocol))
        });
        if chosen.is_none() {
            return Err(if req.socktype != 0 {
                EAI_SOCKTYPE
            } else {
                EAI_SERVICE
            });
        }
    }
    if service.is_some() && chosen.is_some_and(|tp| tp.noservice) {
        return Err(EAI_SERVICE);
    }
    match service {
        None => add_numeric(None, chosen, req, out),
        Some(s) if s.num >= 0 => add_numeric(Some(s.num), chosen, req, out),
        Some(s) => {
            if let Some(tp) = chosen {
                out.push(serv_tuple(s, tp, req)?);
                return Ok(());
            }
            for tp in &TYPEPROTO {
                if tp.noservice {
                    continue;
                }
                if req.socktype != 0 && req.socktype != tp.socktype {
                    continue;
                }
                if req.protocol != 0 && !tp.protoany && req.protocol != tp.protocol {
                    continue;
                }
                if let Ok(t) = serv_tuple(s, tp, req) {
                    out.push(t);
                }
            }
            if out.as_slice().is_empty() {
                return Err(EAI_SERVICE);
            }
            Ok(())
        }
    }
}

/// The numeric-service (or no-service) half of `get_servtuples`: the port
/// is the number's low 16 bits, network order, as `htons` makes it.
fn add_numeric(
    num: Option<i32>,
    chosen: Option<&TypeProto>,
    req: &Req,
    out: &mut List<ServTuple>,
) -> Result<(), i32> {
    let port = num.map_or(0, |n| (n as u16).to_be());
    if let Some(tp) = chosen {
        out.push(ServTuple {
            socktype: tp.socktype,
            protocol: if tp.protoany {
                req.protocol
            } else {
                tp.protocol
            },
            port,
        });
        return Ok(());
    }
    for tp in TYPEPROTO.iter().filter(|tp| tp.default) {
        out.push(ServTuple {
            socktype: tp.socktype,
            protocol: tp.protocol,
            port,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Addresses
// ---------------------------------------------------------------------------

fn is_linklocal(a: &[u8; 16]) -> bool {
    a[0] == 0xfe && a[1] & 0xc0 == 0x80
}

fn is_sitelocal(a: &[u8; 16]) -> bool {
    a[0] == 0xfe && a[1] & 0xc0 == 0xc0
}

fn is_multicast(a: &[u8; 16]) -> bool {
    a[0] == 0xff
}

fn is_mc_nodelocal(a: &[u8; 16]) -> bool {
    is_multicast(a) && a[1] & 0xf == 1
}

fn is_mc_linklocal(a: &[u8; 16]) -> bool {
    is_multicast(a) && a[1] & 0xf == 2
}

fn is_loopback6(a: &[u8; 16]) -> bool {
    a[..15] == [0; 15] && a[15] == 1
}

fn is_v4mapped(a: &[u8; 16]) -> bool {
    a[..10] == [0; 10] && a[10] == 0xff && a[11] == 0xff
}

/// `__inet6_scopeid_pton`: an interface name (for a link- or node-local
/// address) or a decimal number up to `u32::MAX`.
fn scopeid_pton(addr: &[u8; 16], scope: &[u8]) -> Option<u32> {
    if is_linklocal(addr) || is_mc_nodelocal(addr) || is_mc_linklocal(addr) {
        let mut name = [0u8; 64];
        if let Some(n) = name.get_mut(..scope.len()) {
            n.copy_from_slice(scope);
            // SAFETY: `name` is NUL-terminated after the scope.
            let idx = unsafe { crate::socket::if_nametoindex(name.as_ptr()) };
            if idx != 0 {
                return Some(idx);
            }
        }
    }
    if scope.first().is_some_and(u8::is_ascii_digit) && scope.iter().all(u8::is_ascii_digit) {
        let mut v: u64 = 0;
        for &d in scope {
            v = v.saturating_mul(10).saturating_add(u64::from(d - b'0'));
        }
        return u32::try_from(v).ok();
    }
    None
}

/// `text_to_binary_address`: the host as a number, if it is one.
/// `Ok(None)`: not a number, look it up.
fn numeric_host(name: &[u8], req: &Req) -> Result<Option<Tuple>, i32> {
    if let Some(v4) = crate::inet::aton_exact(name) {
        let mut t = Tuple {
            family: AF_INET,
            addr: [0; 16],
            scopeid: 0,
        };
        if req.family == AF_UNSPEC || req.family == AF_INET {
            t.addr[..4].copy_from_slice(&v4);
        } else if req.family == AF_INET6 && req.flags & AI_V4MAPPED != 0 {
            t.family = AF_INET6;
            t.addr = hosts::map_v4(v4);
        } else {
            return Err(EAI_ADDRFAMILY);
        }
        return Ok(Some(t));
    }
    let (text, scope) = match name.iter().position(|&b| b == SCOPE_DELIMITER) {
        Some(i) => (&name[..i], Some(&name[i + 1..])),
        None => (name, None),
    };
    if let Some(a) = crate::inet::pton6(text) {
        let mut t = Tuple {
            family: AF_INET6,
            addr: a,
            scopeid: 0,
        };
        if req.family == AF_UNSPEC || req.family == AF_INET6 {
        } else if req.family == AF_INET && is_v4mapped(&a) {
            t.family = AF_INET;
            t.addr = [0; 16];
            t.addr[..4].copy_from_slice(&a[12..]);
        } else {
            return Err(EAI_ADDRFAMILY);
        }
        if let Some(s) = scope {
            t.scopeid = scopeid_pton(&a, s).ok_or(EAI_NONAME)?;
        }
        return Ok(Some(t));
    }
    if req.flags & AI_NUMERICHOST != 0 {
        return Err(EAI_NONAME);
    }
    Ok(None)
}

/// `get_local_addresses`: for no name, the loopback -- or, with
/// `AI_PASSIVE`, the wildcard -- IPv6 first.
fn local_addresses(req: &Req, out: &mut List<Tuple>) {
    let mut add = |family: i32, loopback: [u8; 16]| {
        out.push(Tuple {
            family,
            addr: if req.flags & AI_PASSIVE == 0 {
                loopback
            } else {
                [0; 16]
            },
            scopeid: 0,
        });
    };
    if req.family == AF_UNSPEC || req.family == AF_INET6 {
        let mut one = [0u8; 16];
        one[15] = 1;
        add(AF_INET6, one);
    }
    if req.family == AF_UNSPEC || req.family == AF_INET {
        let mut lo = [0u8; 16];
        lo[..4].copy_from_slice(&[127, 0, 0, 1]);
        add(AF_INET, lo);
    }
}

// ---------------------------------------------------------------------------
// Looking a name up
// ---------------------------------------------------------------------------

/// What a lookup found: the addresses, the canonical name (a `malloc`ed
/// string, or NULL), and whether an IPv6 address is among them -- glibc's
/// `gaih_result`.
struct Found {
    at: List<Tuple>,
    canon: *mut u8,
    got_ipv6: bool,
}

impl Found {
    fn new() -> Self {
        Self {
            at: List::new(),
            canon: core::ptr::null_mut(),
            got_ipv6: false,
        }
    }

    /// `gaih_result_reset`.
    fn reset(&mut self) {
        self.at.clear();
        // SAFETY: NULL or this result's `strdup`.
        unsafe { crate::malloc::free(self.canon) };
        self.canon = core::ptr::null_mut();
        self.got_ipv6 = false;
    }

    /// Keep the first canonical name offered.
    fn offer_canon(&mut self, name: &[u8]) {
        if self.canon.is_null() {
            self.canon = strdup(name);
        }
    }
}

impl Drop for Found {
    fn drop(&mut self) {
        // SAFETY: NULL or this result's `strdup`.
        unsafe { crate::malloc::free(self.canon) };
    }
}

/// A hosts module's `gethostbyname4_r`: the name, `h_errno`, where the
/// canonical name goes, where each address goes.
type Byname4 = fn(&[u8], &mut i32, &mut dyn FnMut(&[u8]), &mut dyn FnMut(Tuple)) -> Status;

/// A hosts module's `gethostbyname3_r`: as [`Byname4`], for one family.
type Byname3 = fn(&[u8], i32, &mut i32, &mut dyn FnMut(&[u8]), &mut dyn FnMut(Tuple)) -> Status;

/// One hosts module's lookups, for `getaddrinfo`.
struct Module {
    by4: Byname4,
    by3: Byname3,
}

/// `nsswitch.conf`'s `hosts: files dns`, the kernel's resolver being DNS.
const MODULES: [Module; 2] = [
    Module {
        by4: hosts::files_byname4,
        by3: hosts::files_byname3_tuples,
    },
    Module {
        by4: hosts::kernel_byname4,
        by3: hosts::kernel_byname3_tuples,
    },
];

/// `gethosts`: one family's addresses from one module into `res`, IPv4
/// ones mapped when the question is IPv6.  Its status and "no data" verdict.
fn gethosts(
    m: &Module,
    family: i32,
    name: &[u8],
    req: &Req,
    res: &mut Found,
    herr: &mut i32,
) -> (Status, i32) {
    let mut canon: Option<List<u8>> = None;
    let mut added = List::<Tuple>::new();
    let status = (m.by3)(
        name,
        family,
        herr,
        &mut |c| {
            let mut l = List::new();
            for &b in c {
                l.push(b);
            }
            canon = Some(l);
        },
        &mut |t| added.push(t),
    );
    let mut no_data = 0;
    match status {
        Status::NotFound | Status::TryAgain | Status::Unavail => {
            if *herr == TRY_AGAIN {
                no_data = EAI_AGAIN;
            } else {
                no_data = i32::from(*herr == NO_DATA);
            }
        }
        Status::Success => {
            for &t in added.as_slice() {
                let mut t = t;
                if family == AF_INET && req.family == AF_INET6 {
                    let mut v4 = [0u8; 4];
                    v4.copy_from_slice(&t.addr[..4]);
                    t.family = AF_INET6;
                    t.addr = hosts::map_v4(v4);
                }
                res.at.push(t);
            }
            if !added.as_slice().is_empty() {
                res.got_ipv6 = family == AF_INET6;
            }
            // glibc asks `gethostbyname3_r`, which reports the name, only
            // for `AI_CANONNAME`; `gethostbyname2_r` reports none.
            if req.flags & AI_CANONNAME != 0 {
                if let Some(c) = &canon {
                    res.offer_canon(c.as_slice());
                }
            }
        }
    }
    (status, no_data)
}

/// `get_nss_addresses`: the modules in order until one answers, as glibc
/// asks them -- all addresses at once for `AF_UNSPEC`, otherwise IPv6 then
/// IPv4 as the question needs.  `Err` is an `EAI_*` code.
fn nss_addresses(name: &[u8], req: &Req, res: &mut Found) -> Result<(), i32> {
    let mut herr = crate::socket::get_h_errno();
    let mut no_data = 0;
    let mut no_inet6_data = 0;
    let mut status = Status::Unavail;
    for m in &MODULES {
        res.reset();
        no_data = 0;
        if req.family == AF_UNSPEC {
            let mut canon: Option<List<u8>> = None;
            let mut all = List::<Tuple>::new();
            status = (m.by4)(
                name,
                &mut herr,
                &mut |c| {
                    let mut l = List::new();
                    for &b in c {
                        l.push(b);
                    }
                    canon = Some(l);
                },
                &mut |t| all.push(t),
            );
            if status == Status::Success {
                no_data = 1;
                if req.flags & AI_CANONNAME != 0 {
                    if let Some(c) = &canon {
                        res.offer_canon(c.as_slice());
                    }
                }
                for &t in all.as_slice() {
                    res.at.push(t);
                    no_data = 0;
                }
            } else if herr == TRY_AGAIN {
                no_data = EAI_AGAIN;
            } else {
                no_data = i32::from(herr == NO_DATA);
            }
            no_inet6_data = no_data;
        } else {
            let mut inet6_status = Status::Unavail;
            if req.family == AF_INET6 {
                let (s, nd) = gethosts(m, AF_INET6, name, req, res, &mut herr);
                no_data = nd;
                no_inet6_data = nd;
                inet6_status = s;
                status = s;
            }
            if req.family == AF_INET
                || (req.family == AF_INET6
                    && req.flags & AI_V4MAPPED != 0
                    && (req.flags & AI_ALL != 0 || !res.got_ipv6))
            {
                let (s, nd) = gethosts(m, AF_INET, name, req, res, &mut herr);
                status = s;
                no_data = nd;
                if req.family == AF_INET {
                    no_inet6_data = nd;
                    inet6_status = s;
                }
            }
            if inet6_status == Status::Success || status == Status::Success {
                if req.flags & AI_CANONNAME != 0 && res.canon.is_null() {
                    res.canon = strdup(name);
                    if res.canon.is_null() {
                        return Err(EAI_MEMORY);
                    }
                }
                status = Status::Success;
            } else if inet6_status == Status::TryAgain {
                status = Status::TryAgain;
            } else if status == Status::Unavail && inet6_status != Status::Unavail {
                status = inet6_status;
            }
        }
        if res.at.failed {
            return Err(EAI_MEMORY);
        }
        // `[SUCCESS=return]`, and every other status goes on to the next.
        if status == Status::Success {
            break;
        }
    }
    crate::socket::set_h_errno(herr);
    if matches!(status, Status::TryAgain | Status::Unavail) && herr == NETDB_INTERNAL {
        return Err(EAI_SYSTEM);
    }
    if no_data != 0 && no_inet6_data != 0 {
        if no_data == EAI_AGAIN && no_inet6_data == EAI_AGAIN {
            return Err(EAI_AGAIN);
        }
        return Err(EAI_NODATA);
    }
    if status != Status::Success {
        res.at.clear();
    }
    Ok(())
}

/// `try_simple_gethostbyname`: an `AF_INET` question without
/// `AI_CANONNAME` is `gethostbyname2_r`'s, whose answers and errors are
/// its own.
fn simple_lookup(name: &[u8], req: &Req, res: &mut Found) -> Result<bool, i32> {
    if req.family != AF_INET || req.flags & AI_CANONNAME != 0 {
        return Ok(false);
    }
    let mut cname = List::<u8>::new();
    for &b in name {
        cname.push(b);
    }
    cname.push(0);
    if cname.failed {
        return Err(EAI_MEMORY);
    }
    let mut len = 1024usize;
    loop {
        let buf = crate::malloc::malloc(len);
        if buf.is_null() {
            return Err(EAI_MEMORY);
        }
        let mut h = crate::socket::Hostent {
            h_name: core::ptr::null(),
            h_aliases: core::ptr::null(),
            h_addrtype: 0,
            h_length: 0,
            h_addr_list: core::ptr::null(),
        };
        let mut result: *const crate::socket::Hostent = core::ptr::null();
        let mut herr = crate::socket::get_h_errno();
        // SAFETY: a NUL-terminated name; outputs and `len` bytes this
        // function owns.
        let rc = unsafe {
            hosts::gethostbyname2_r(
                cname.as_slice().as_ptr(),
                AF_INET,
                &mut h,
                buf,
                len,
                &mut result,
                &mut herr,
            )
        };
        if rc == errno::ERANGE && herr == NETDB_INTERNAL {
            // SAFETY: this function's block.
            unsafe { crate::malloc::free(buf) };
            len *= 2;
            continue;
        }
        let answer = if rc == 0 && !result.is_null() {
            let mut i = 0usize;
            // SAFETY: the answer's NULL-terminated address list, four bytes
            // each, in `buf`.
            unsafe {
                while !(*h.h_addr_list.add(i)).is_null() {
                    let a = *h.h_addr_list.add(i);
                    let mut t = Tuple {
                        family: AF_INET,
                        addr: [0; 16],
                        scopeid: 0,
                    };
                    core::ptr::copy_nonoverlapping(a, t.addr.as_mut_ptr(), 4);
                    res.at.push(t);
                    i += 1;
                }
            }
            Ok(true)
        } else if rc == 0 {
            Err(if herr == NO_DATA {
                EAI_NODATA
            } else {
                EAI_NONAME
            })
        } else if herr == NETDB_INTERNAL {
            Err(EAI_SYSTEM)
        } else if herr == TRY_AGAIN {
            Err(EAI_AGAIN)
        } else {
            Err(EAI_NODATA)
        };
        // SAFETY: this function's block; the addresses are copied out.
        unsafe { crate::malloc::free(buf) };
        crate::socket::set_h_errno(herr);
        if res.at.failed {
            return Err(EAI_MEMORY);
        }
        return answer;
    }
}

// ---------------------------------------------------------------------------
// The answer
// ---------------------------------------------------------------------------

/// `generate_addrinfo`: one `malloc`ed `struct addrinfo` per address and
/// socket type, its socket address after it in the same block, linked in
/// order onto `*tail`; the canonical name on the first.  The number of
/// addresses used, or `EAI_MEMORY` -- after which the list so far is still
/// linked and NULL-terminated, for the caller to free.
fn generate(
    req: &Req,
    res: &mut Found,
    st: &[ServTuple],
    mut tail: *mut *mut Addrinfo,
) -> Result<usize, i32> {
    let mut naddrs = 0usize;
    for at in res.at.as_slice() {
        let family = at.family;
        let socklen = if family == AF_INET6 {
            // Mapped IPv4 answers are dropped when the caller did not ask
            // for all of them and an IPv6 one was found.
            if res.got_ipv6
                && req.flags & (AI_V4MAPPED | AI_ALL) == AI_V4MAPPED
                && is_v4mapped(&at.addr)
            {
                continue;
            }
            size_of::<SockaddrIn6>()
        } else {
            size_of::<SockaddrIn>()
        };
        for s in st {
            let block = crate::malloc::malloc(size_of::<Addrinfo>() + socklen);
            if block.is_null() {
                return Err(EAI_MEMORY);
            }
            let ai = block.cast::<Addrinfo>();
            // SAFETY: a fresh block holding an `Addrinfo` (malloc aligns it)
            // and `socklen` bytes after it, where the address goes; `Addrinfo`
            // is pointer-aligned and a multiple of 8 long, so the address is
            // aligned too.
            unsafe {
                let sa = block.add(size_of::<Addrinfo>());
                if family == AF_INET6 {
                    sa.cast::<SockaddrIn6>().write(SockaddrIn6 {
                        sin6_family: AF_INET6 as u16,
                        sin6_port: s.port,
                        sin6_flowinfo: 0,
                        sin6_addr: In6Addr { s6_addr: at.addr },
                        sin6_scope_id: at.scopeid,
                    });
                } else {
                    let mut v4 = [0u8; 4];
                    v4.copy_from_slice(&at.addr[..4]);
                    sa.cast::<SockaddrIn>().write(SockaddrIn {
                        sin_family: AF_INET as u16,
                        sin_port: s.port,
                        sin_addr: InAddr {
                            s_addr: u32::from_ne_bytes(v4),
                        },
                        sin_zero: [0; 8],
                    });
                }
                ai.write(Addrinfo {
                    ai_flags: req.flags,
                    ai_family: family,
                    ai_socktype: s.socktype,
                    ai_protocol: s.protocol,
                    ai_addrlen: socklen as SocklenT,
                    // Only the first entry gets the name.
                    ai_canonname: core::mem::replace(&mut res.canon, core::ptr::null_mut()),
                    ai_addr: sa.cast::<Sockaddr>(),
                    ai_next: core::ptr::null_mut(),
                });
                *tail = ai;
                tail = &raw mut (*ai).ai_next;
            }
        }
        naddrs += 1;
    }
    Ok(naddrs)
}

/// Free an answer [`getaddrinfo`] gave: every entry, and the canonical
/// name on whichever carries it.  NULL is nothing to free.
///
/// # Safety
///
/// `ai` is NULL, or a list `getaddrinfo` returned (or its tail), not freed
/// already.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn freeaddrinfo(ai: *mut Addrinfo) {
    let mut p = ai;
    while !p.is_null() {
        // SAFETY: a live entry of the list, per the contract; its next
        // pointer and name are read before it is freed.
        unsafe {
            let next = (*p).ai_next;
            crate::malloc::free((*p).ai_canonname);
            crate::malloc::free(p.cast());
            p = next;
        }
    }
}

/// The message for an `EAI_*` code: glibc's, `"Unknown error"` for any other.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gai_strerror(code: i32) -> *const u8 {
    let m: &core::ffi::CStr = match code {
        0 => c"Success",
        EAI_ADDRFAMILY => c"Address family for hostname not supported",
        EAI_AGAIN => c"Temporary failure in name resolution",
        EAI_BADFLAGS => c"Bad value for ai_flags",
        crate::socket::EAI_FAIL => c"Non-recoverable failure in name resolution",
        EAI_FAMILY => c"ai_family not supported",
        EAI_MEMORY => c"Memory allocation failure",
        EAI_NODATA => c"No address associated with hostname",
        EAI_NONAME => c"Name or service not known",
        EAI_SERVICE => c"Servname not supported for ai_socktype",
        EAI_SOCKTYPE => c"ai_socktype not supported",
        EAI_SYSTEM => c"System error",
        -100 => c"Processing request in progress",
        -101 => c"Request canceled",
        -102 => c"Request not canceled",
        -103 => c"All requests done",
        -104 => c"Interrupted by a signal",
        EAI_IDN_ENCODE => c"Parameter string not correctly encoded",
        crate::socket::EAI_OVERFLOW => c"Result too large for supplied buffer",
        _ => c"Unknown error",
    };
    m.as_ptr().cast()
}

// ---------------------------------------------------------------------------
// Sorting: RFC 3484 with glibc's tables, or /etc/gai.conf's
// ---------------------------------------------------------------------------

/// A prefix-table entry: `label` and `precedence` lines.
#[derive(Clone, Copy)]
struct PrefixEntry {
    prefix: [u8; 16],
    bits: u32,
    val: i32,
}

/// An IPv4 scope entry: `scopev4` lines.  `addr` and `netmask` are network
/// byte order, compared as glibc compares them.
#[derive(Clone, Copy)]
struct ScopeEntry {
    addr: u32,
    netmask: u32,
    scope: i32,
}

const fn prefix(p: [u8; 16], bits: u32, val: i32) -> PrefixEntry {
    PrefixEntry {
        prefix: p,
        bits,
        val,
    }
}

const ZERO16: [u8; 16] = [0; 16];

/// glibc's `default_labels`.
const DEFAULT_LABELS: [PrefixEntry; 8] = [
    prefix([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1], 128, 0),
    prefix(
        [0x20, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        16,
        2,
    ),
    prefix(ZERO16, 96, 3),
    prefix(
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 0, 0, 0, 0],
        96,
        4,
    ),
    prefix(
        [0xfe, 0xc0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        10,
        5,
    ),
    prefix([0xfc, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 7, 6),
    prefix(
        [0x20, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        32,
        7,
    ),
    prefix(ZERO16, 0, 1),
];

/// glibc's `default_precedence`.
const DEFAULT_PRECEDENCE: [PrefixEntry; 5] = [
    prefix([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1], 128, 50),
    prefix(
        [0x20, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        16,
        30,
    ),
    prefix(ZERO16, 96, 20),
    prefix(
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 0, 0, 0, 0],
        96,
        10,
    ),
    prefix(ZERO16, 0, 40),
];

/// glibc's `default_scopes`: link-local and loopback IPv4 are scope 2,
/// the rest 14.
const DEFAULT_SCOPES: [ScopeEntry; 3] = [
    ScopeEntry {
        addr: u32::from_ne_bytes([169, 254, 0, 0]),
        netmask: u32::from_ne_bytes([255, 255, 0, 0]),
        scope: 2,
    },
    ScopeEntry {
        addr: u32::from_ne_bytes([127, 0, 0, 0]),
        netmask: u32::from_ne_bytes([255, 0, 0, 0]),
        scope: 2,
    },
    ScopeEntry {
        addr: 0,
        netmask: 0,
        scope: 14,
    },
];

/// The tables in force: glibc's, or `/etc/gai.conf`'s.
struct Tables {
    labels: List<PrefixEntry>,
    precedence: List<PrefixEntry>,
    scopes: List<ScopeEntry>,
    /// `reload yes`: look at the file's modification time on every call.
    reload: bool,
    /// The file's modification time when it was read (seconds,
    /// nanoseconds), or `None` when there was none.
    mtime: Option<(i64, i64)>,
}

impl Tables {
    const fn new() -> Self {
        Self {
            labels: List::new(),
            precedence: List::new(),
            scopes: List::new(),
            reload: false,
            mtime: None,
        }
    }

    fn labels(&self) -> &[PrefixEntry] {
        let l = self.labels.as_slice();
        if l.is_empty() { &DEFAULT_LABELS } else { l }
    }

    fn precedence(&self) -> &[PrefixEntry] {
        let l = self.precedence.as_slice();
        if l.is_empty() { &DEFAULT_PRECEDENCE } else { l }
    }

    fn scopes(&self) -> &[ScopeEntry] {
        let l = self.scopes.as_slice();
        if l.is_empty() { &DEFAULT_SCOPES } else { l }
    }
}

/// The destination and source addresses sorting compares: IPv4 as an
/// IPv4-mapped IPv6 address, as glibc's `match_prefix` makes it.
#[derive(Clone, Copy)]
struct Sa {
    family: i32,
    /// For `AF_INET`, the four bytes in `addr[..4]`.
    addr: [u8; 16],
}

impl Sa {
    fn as_v6(&self) -> [u8; 16] {
        if self.family == AF_INET {
            let mut v4 = [0u8; 4];
            v4.copy_from_slice(&self.addr[..4]);
            hosts::map_v4(v4)
        } else {
            self.addr
        }
    }

    fn v4_word(&self) -> u32 {
        u32::from_ne_bytes([self.addr[0], self.addr[1], self.addr[2], self.addr[3]])
    }
}

/// `match_prefix`: the value of the first entry whose prefix `sa` is in;
/// `default` for a family that is neither.
fn match_prefix(sa: &Sa, list: &[PrefixEntry], default: i32) -> i32 {
    if sa.family != AF_INET && sa.family != AF_INET6 {
        return default;
    }
    let a = sa.as_v6();
    for e in list {
        let full = (e.bits / 8) as usize;
        let rest = e.bits % 8;
        if e.prefix[..full.min(16)] != a[..full.min(16)] {
            continue;
        }
        if rest == 0 || full >= 16 {
            return e.val;
        }
        let mask = (0xff00u32 >> rest) as u8;
        if e.prefix[full] & mask == a[full] & mask {
            return e.val;
        }
    }
    default
}

/// `get_scope`.
fn get_scope(sa: &Sa, t: &Tables) -> i32 {
    if sa.family == AF_INET6 {
        let a = &sa.addr;
        if !is_multicast(a) {
            if is_linklocal(a) || is_loopback6(a) {
                2
            } else if is_sitelocal(a) {
                5
            } else {
                14
            }
        } else {
            i32::from(a[1] & 0xf)
        }
    } else if sa.family == AF_INET {
        let w = sa.v4_word();
        for s in t.scopes() {
            if w & s.netmask == s.addr {
                return s.scope;
            }
        }
        14
    } else {
        15
    }
}

/// `fls`: the number of leading zero bits.
fn fls(a: u32) -> i32 {
    a.leading_zeros() as i32
}

/// One entry being sorted: glibc's `sort_result`.
#[derive(Clone, Copy)]
struct SortEntry {
    dest: Sa,
    src: Sa,
    got_source: bool,
    prefixlen: u32,
}

/// `rfc3484_sort`: whether entry `i` goes before entry `j` (rule 10:
/// otherwise their order stays).
fn rfc3484_before(e: &[SortEntry], i: usize, j: usize, t: &Tables) -> bool {
    let (a1, a2) = (&e[i], &e[j]);
    // Rule 1: avoid unusable destinations.
    if a1.got_source != a2.got_source {
        return a1.got_source;
    }
    // Rule 2: prefer matching scope.
    let a1_dst_scope = get_scope(&a1.dest, t);
    let a2_dst_scope = get_scope(&a2.dest, t);
    if a1.got_source {
        let a1_src_scope = get_scope(&a1.src, t);
        let a2_src_scope = get_scope(&a2.src, t);
        if a1_dst_scope == a1_src_scope && a2_dst_scope != a2_src_scope {
            return true;
        }
        if a1_dst_scope != a1_src_scope && a2_dst_scope == a2_src_scope {
            return false;
        }
    }
    // Rules 3 and 4 need the kernel's per-address flags, which nothing here
    // has: every address is neither deprecated nor a home address.
    // Rule 5: prefer matching label.
    if a1.got_source {
        let l = t.labels();
        let a1_dst = match_prefix(&a1.dest, l, i32::MAX);
        let a1_src = match_prefix(&a1.src, l, i32::MAX);
        let a2_dst = match_prefix(&a2.dest, l, i32::MAX);
        let a2_src = match_prefix(&a2.src, l, i32::MAX);
        if a1_dst == a1_src && a2_dst != a2_src {
            return true;
        }
        if a1_dst != a1_src && a2_dst == a2_src {
            return false;
        }
    }
    // Rule 6: prefer higher precedence.
    let p = t.precedence();
    let a1_prec = match_prefix(&a1.dest, p, 0);
    let a2_prec = match_prefix(&a2.dest, p, 0);
    if a1_prec != a2_prec {
        return a1_prec > a2_prec;
    }
    // Rule 7 compares interfaces, and every source here has the same
    // unknown one: it never decides.
    // Rule 8: prefer smaller scope.
    if a1_dst_scope != a2_dst_scope {
        return a1_dst_scope < a2_dst_scope;
    }
    // Rule 9: use longest matching prefix.
    if a1.got_source && a1.dest.family == a2.dest.family {
        let (mut bit1, mut bit2) = (0, 0);
        if a1.dest.family == AF_INET {
            // Only on the same subnet, by the source's prefix length; with
            // none known (0), glibc's `0xffffffff << 32` is x86's
            // `0xffffffff`.
            let net = |x: &SortEntry| -> i32 {
                let dst = u32::from_be(x.dest.v4_word());
                let src = u32::from_be(x.src.v4_word());
                let mask = 0xffff_ffffu32.wrapping_shl(32u32.wrapping_sub(x.prefixlen));
                if src & mask == dst & mask {
                    fls(dst ^ src)
                } else {
                    0
                }
            };
            bit1 = net(a1);
            bit2 = net(a2);
        } else if a1.dest.family == AF_INET6 {
            let word = |a: &[u8; 16], k: usize| {
                u32::from_be_bytes([a[k * 4], a[k * 4 + 1], a[k * 4 + 2], a[k * 4 + 3]])
            };
            let k = (0..4).find(|&k| {
                word(&a1.dest.addr, k) != word(&a1.src.addr, k)
                    || word(&a2.dest.addr, k) != word(&a2.src.addr, k)
            });
            if let Some(k) = k {
                bit1 = fls(word(&a1.dest.addr, k) ^ word(&a1.src.addr, k));
                bit2 = fls(word(&a2.dest.addr, k) ^ word(&a2.src.addr, k));
            }
        }
        if bit1 != bit2 {
            return bit1 > bit2;
        }
    }
    // Rule 10: leave the order unchanged.
    i < j
}

/// The source address the system would use to reach `dest`, as glibc finds
/// it: connect a datagram socket and ask for its name.  `None`: unusable.
fn source_address(dest: &Sa) -> Option<Sa> {
    use crate::socket::{SOCK_CLOEXEC, connect, getsockname, socket};
    let fd = socket(dest.family, SOCK_DGRAM | SOCK_CLOEXEC, 0);
    if fd < 0 {
        return None;
    }
    let mut out = None;
    if dest.family == AF_INET {
        let sin = SockaddrIn {
            sin_family: AF_INET as u16,
            sin_port: 0,
            sin_addr: InAddr {
                s_addr: dest.v4_word(),
            },
            sin_zero: [0; 8],
        };
        // SAFETY: `sin` is a live `sockaddr_in`; `name` is written by
        // `getsockname` within the length it is given.
        unsafe {
            if connect(
                fd,
                (&raw const sin).cast(),
                size_of::<SockaddrIn>() as SocklenT,
            ) == 0
            {
                let mut name = SockaddrIn {
                    sin_family: 0,
                    sin_port: 0,
                    sin_addr: InAddr { s_addr: 0 },
                    sin_zero: [0; 8],
                };
                let mut len = size_of::<SockaddrIn>() as SocklenT;
                if getsockname(fd, (&raw mut name).cast(), &mut len) == 0 {
                    let mut a = [0u8; 16];
                    a[..4].copy_from_slice(&name.sin_addr.s_addr.to_ne_bytes());
                    out = Some(Sa {
                        family: AF_INET,
                        addr: a,
                    });
                }
            }
        }
    }
    // A datagram socket that was only connected has nothing to lose.
    let _ = crate::file::close(fd);
    out
}

/// Sort the answer, as glibc does whenever there is more than one address.
/// `list` is the answer's head; the new head is returned, the canonical
/// name moved to it.
fn sort(list: *mut Addrinfo, t: &Tables) -> Result<*mut Addrinfo, i32> {
    let mut nodes = List::<*mut Addrinfo>::new();
    let mut entries = List::<SortEntry>::new();
    let mut canon: *mut u8 = core::ptr::null_mut();
    let mut p = list;
    let mut last: Option<SortEntry> = None;
    while !p.is_null() {
        // SAFETY: a live entry of the answer being built.
        let ai = unsafe { &mut *p };
        let dest = sa_of(ai);
        // The same address again (another socket type): the same source.
        let entry = match last {
            Some(l) if l.dest.family == dest.family && l.dest.addr == dest.addr => {
                SortEntry { dest, ..l }
            }
            _ => {
                let src = source_address(&dest);
                SortEntry {
                    dest,
                    src: src.unwrap_or(dest),
                    got_source: src.is_some(),
                    prefixlen: 0,
                }
            }
        };
        last = Some(entry);
        if !ai.ai_canonname.is_null() {
            canon = core::mem::replace(&mut ai.ai_canonname, core::ptr::null_mut());
        }
        nodes.push(p);
        entries.push(entry);
        p = ai.ai_next;
    }
    if nodes.failed || entries.failed {
        // Put the name back where it was; the caller frees the list.
        // SAFETY: `list` is the answer's live head.
        unsafe { (*list).ai_canonname = canon };
        return Err(EAI_MEMORY);
    }
    let n = entries.as_slice().len();
    let mut order = List::<usize>::new();
    for i in 0..n {
        order.push(i);
    }
    if order.failed {
        // SAFETY: as above.
        unsafe { (*list).ai_canonname = canon };
        return Err(EAI_MEMORY);
    }
    // A stable insertion sort: the comparison ends in "keep the order", so
    // any correct sort gives glibc's result.
    let e = entries.as_slice();
    let ord = order.as_mut_slice();
    for i in 1..n {
        let mut k = i;
        while k > 0 && rfc3484_before(e, ord[k], ord[k - 1], t) {
            ord.swap(k, k - 1);
            k -= 1;
        }
    }
    let heads = nodes.as_slice();
    let head = heads[ord[0]];
    // SAFETY: every node is a live entry of the answer; relinked in order.
    unsafe {
        let mut q = head;
        for &i in &ord[1..] {
            (*q).ai_next = heads[i];
            q = heads[i];
        }
        (*q).ai_next = core::ptr::null_mut();
        (*head).ai_canonname = canon;
    }
    Ok(head)
}

/// An answer entry's address, for sorting.
fn sa_of(ai: &Addrinfo) -> Sa {
    let mut a = [0u8; 16];
    // SAFETY: `ai_addr` is the entry's own address, of its family's size.
    unsafe {
        if ai.ai_family == AF_INET6 {
            a = (*ai.ai_addr.cast::<SockaddrIn6>()).sin6_addr.s6_addr;
        } else {
            a[..4].copy_from_slice(
                &(*ai.ai_addr.cast::<SockaddrIn>())
                    .sin_addr
                    .s_addr
                    .to_ne_bytes(),
            );
        }
    }
    Sa {
        family: ai.ai_family,
        addr: a,
    }
}

// ---------------------------------------------------------------------------
// /etc/gai.conf
// ---------------------------------------------------------------------------

/// `strtoul (s, &end, 10)` over a whole field: the value, or `None` when
/// the field is not all a number (`*end != '\0'`) or overflows.
fn strtoul_field(s: &[u8]) -> Option<u64> {
    let (v, used) = crate::netdb::strtou64(s, 10)?;
    (used == s.len()).then_some(v)
}

/// `add_prefixlist`: `prefix[/bits] value` into `list`, when it reads.  A
/// prefix with no `/bits` is 128 bits long (glibc reads an uninitialised
/// pointer there; this is what it means to do).
fn add_prefix(list: &mut List<PrefixEntry>, nullbits: &mut bool, val1: &[u8], val2: &[u8]) {
    let (text, bits) = match val1.iter().position(|&b| b == b'/') {
        Some(i) => (&val1[..i], Some(&val1[i + 1..])),
        None => (val1, None),
    };
    let Some(p) = crate::inet::pton6(text) else {
        return;
    };
    let bits = match bits {
        Some(b) => match strtoul_field(b) {
            Some(v) => v,
            None => return,
        },
        None => 128,
    };
    if bits > 128 {
        return;
    }
    let Some(val) = strtoul_field(val2).filter(|&v| v <= i32::MAX as u64) else {
        return;
    };
    list.push(PrefixEntry {
        prefix: p,
        bits: bits as u32,
        val: val as i32,
    });
    *nullbits |= bits == 0;
}

/// `add_scopelist`, after `scopev4`'s own parsing: an IPv4 prefix -- as an
/// IPv4-mapped IPv6 one of 96 to 128 bits, or plain of up to 32 -- and its
/// scope.
fn add_scope(list: &mut List<ScopeEntry>, nullbits: &mut bool, val1: &[u8], val2: &[u8]) {
    let (text, bits_text) = match val1.iter().position(|&b| b == b'/') {
        Some(i) => (&val1[..i], Some(&val1[i + 1..])),
        None => (val1, None),
    };
    let val = || strtoul_field(val2).filter(|&v| v <= i32::MAX as u64);
    let (v4, bits) = if let Some(p) = crate::inet::pton6(text) {
        let bits = match bits_text {
            Some(b) => match strtoul_field(b) {
                Some(v) => v,
                None => return,
            },
            None => 128,
        };
        if !is_v4mapped(&p) || !(96..=128).contains(&bits) {
            return;
        }
        ([p[12], p[13], p[14], p[15]], bits)
    } else if let Some(a) = crate::inet::pton4(text) {
        let bits = match bits_text {
            Some(b) => match strtoul_field(b) {
                Some(v) => v,
                None => return,
            },
            None => 32,
        };
        if bits > 32 {
            return;
        }
        (a, bits + 96)
    } else {
        return;
    };
    let Some(scope) = val() else {
        return;
    };
    let mask_host = if bits == 96 {
        0
    } else {
        0xffff_ffffu32 << (128 - bits)
    };
    let netmask = u32::from_ne_bytes(mask_host.to_be_bytes());
    list.push(ScopeEntry {
        addr: u32::from_ne_bytes(v4) & netmask,
        netmask,
        scope: scope as i32,
    });
    *nullbits |= bits == 96;
}

/// `gaiconf_init`: the tables `/etc/gai.conf` sets, or none (glibc's) when
/// it cannot be read.
fn load_tables() -> Tables {
    let mut t = Tables::new();
    let Ok(crate::nss_files::Db::Text(text)) =
        crate::nss_files::read(crate::nss_files::Which::GaiConf)
    else {
        return t;
    };
    t.mtime = gaiconf_mtime();
    let (mut labels, mut prec, mut scopes) = (List::new(), List::new(), List::new());
    let (mut lnull, mut pnull, mut snull) = (false, false, false);
    for line in text.bytes().split_inclusive(|&b| b == b'\n') {
        // A comment runs to the end of the line; a NUL ends it as in C.
        let end = line
            .iter()
            .position(|&b| b == b'#' || b == 0)
            .unwrap_or(line.len());
        let line = &line[..end];
        let mut words = line
            .split(|&b| crate::nss_files::is_space(b))
            .filter(|w| !w.is_empty());
        let Some(cmd) = words.next() else {
            continue;
        };
        let val1 = words.next().unwrap_or(&[]);
        let val2 = words.next().unwrap_or(&[]);
        match cmd {
            b"label" => add_prefix(&mut labels, &mut lnull, val1, val2),
            b"reload" => t.reload = val1 == b"yes",
            b"scopev4" => add_scope(&mut scopes, &mut snull, val1, val2),
            b"precedence" => add_prefix(&mut prec, &mut pnull, val1, val2),
            _ => {}
        }
    }
    if labels.failed || prec.failed || scopes.failed {
        // glibc falls back to its own tables when memory runs out.
        let reload = t.reload;
        let mut fresh = Tables::new();
        fresh.reload = reload;
        return fresh;
    }
    // The catch-all entry each list needs, unless the file gave one; then
    // the most specific first, the file's order kept among equals.
    if !labels.as_slice().is_empty() && !lnull {
        labels.push(prefix(ZERO16, 0, 1));
    }
    if !prec.as_slice().is_empty() && !pnull {
        prec.push(prefix(ZERO16, 0, 40));
    }
    if !scopes.as_slice().is_empty() && !snull {
        scopes.push(ScopeEntry {
            addr: 0,
            netmask: 0,
            scope: 14,
        });
    }
    stable_sort_by_key(labels.as_mut_slice(), |e| core::cmp::Reverse(e.bits));
    stable_sort_by_key(prec.as_mut_slice(), |e| core::cmp::Reverse(e.bits));
    stable_sort_by_key(scopes.as_mut_slice(), |e| core::cmp::Reverse(e.netmask));
    t.labels = labels;
    t.precedence = prec;
    t.scopes = scopes;
    t
}

/// A stable insertion sort (the lists are a few lines of a file).
fn stable_sort_by_key<T: Copy, K: Ord>(v: &mut [T], key: impl Fn(&T) -> K) {
    for i in 1..v.len() {
        let mut k = i;
        while k > 0 && key(&v[k]) < key(&v[k - 1]) {
            v.swap(k, k - 1);
            k -= 1;
        }
    }
}

/// `/etc/gai.conf`'s modification time, or `None`.
fn gaiconf_mtime() -> Option<(i64, i64)> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        // SAFETY: `Stat` is integers, for which zero is valid.
        let mut st: crate::stat::Stat = unsafe { core::mem::zeroed() };
        if crate::file::stat(b"/etc/gai.conf\0".as_ptr(), &mut st) != 0 {
            return None;
        }
        Some((st.st_mtim.tv_sec, st.st_mtim.tv_nsec))
    }
}

crate::perprocess::process_global! {
    /// The process's tables: read once, as glibc reads them (`__libc_once`).
    fn tables_cell() -> (core::sync::atomic::AtomicU8, core::cell::UnsafeCell<Tables>) =
        (core::sync::atomic::AtomicU8::new(0), core::cell::UnsafeCell::new(Tables::new()));
}

/// Run `f` with the tables in force: the process's, read on first use --
/// or, with `reload yes`, the file's current ones when it has changed.
fn with_tables<R>(f: impl FnOnce(&Tables) -> R) -> R {
    use core::sync::atomic::Ordering;
    // SAFETY: the process's cell, which lives forever.
    let cell = unsafe { &*tables_cell() };
    if cell.0.load(Ordering::Acquire) != 2 {
        if cell
            .0
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Acquire)
            .is_ok()
        {
            // SAFETY: this thread won the right to write, and nobody reads
            // until the state is 2.
            unsafe { *cell.1.get() = load_tables() };
            cell.0.store(2, Ordering::Release);
        } else {
            while cell.0.load(Ordering::Acquire) != 2 {
                let _ = crate::pthread::sched_yield();
            }
        }
    }
    // SAFETY: the state is 2: written once, never again.
    let t = unsafe { &*cell.1.get() };
    if t.reload && gaiconf_mtime() != t.mtime {
        // Changed since: read it again, for this call.
        let fresh = load_tables();
        return f(&fresh);
    }
    f(t)
}

/// Read `/etc/gai.conf` again on this thread's next sort.  Host tests only.
#[cfg(test)]
fn reset_tables() {
    // SAFETY: the thread's own cell on the host.
    unsafe {
        (*tables_cell())
            .0
            .store(0, core::sync::atomic::Ordering::Release)
    };
}

// ---------------------------------------------------------------------------
// getaddrinfo
// ---------------------------------------------------------------------------

/// `__check_pf` for `AI_ADDRCONFIG`: whether this machine has an IPv4
/// address other than 127.0.0.1, and an IPv6 one other than `::1`.  It has
/// no IPv6 addresses at all.
fn check_pf() -> (bool, bool) {
    let mut v4 = false;
    crate::socket::for_each_ipv4_interface(|addr, _| {
        if addr != u32::from_ne_bytes([127, 0, 0, 1]) {
            v4 = true;
        }
    });
    (v4, false)
}

/// `gaih_inet`: the answer for a name and service, onto `*head`.  The
/// number of addresses, or an `EAI_*` code.
fn gaih_inet(
    name: Option<&[u8]>,
    service: Option<&Service>,
    req: &Req,
    head: *mut *mut Addrinfo,
) -> Result<usize, i32> {
    let mut st = List::<ServTuple>::new();
    servtuples(service, req, &mut st)?;
    if st.failed {
        return Err(EAI_MEMORY);
    }
    let mut res = Found::new();
    match name {
        None => local_addresses(req, &mut res.at),
        Some(orig) => {
            // No `libidn2`: an ASCII name is itself, any other cannot be
            // encoded.
            if req.flags & AI_IDN != 0 && orig.iter().any(|&b| b >= 0x80) {
                return Err(EAI_IDN_ENCODE);
            }
            if let Some(t) = numeric_host(orig, req)? {
                res.at.push(t);
                if req.flags & AI_CANONNAME != 0 {
                    res.canon = strdup(orig);
                    if res.canon.is_null() {
                        return Err(EAI_MEMORY);
                    }
                }
            } else if !simple_lookup(orig, req, &mut res)? {
                nss_addresses(orig, req, &mut res)?;
                if res.at.as_slice().is_empty() {
                    return Err(EAI_NONAME);
                }
            }
            // `process_canonname`: the name asked, when nothing better was
            // found; `AI_CANONIDN` has no `libidn2` to decode it with.
            if req.flags & AI_CANONNAME != 0 && res.canon.is_null() {
                res.canon = strdup(orig);
                if res.canon.is_null() {
                    return Err(EAI_MEMORY);
                }
            }
        }
    }
    if res.at.failed {
        return Err(EAI_MEMORY);
    }
    generate(req, &mut res, st.as_slice(), head)
}

/// `strtoul (service, &c, 10)`, as `getaddrinfo` reads a service: leading
/// white space and a sign allowed, the `unsigned long` kept in an `int`.
/// `None`: not all of it a number.
fn service_number(s: &[u8]) -> Option<i32> {
    let (v, used) = crate::netdb::strtou64(s, 10)?;
    (used == s.len()).then_some(v as u32 as i32)
}

/// Translate a host and service into socket addresses: glibc's
/// `getaddrinfo`.  0 and the answer in `*pai`, or an `EAI_*` code.
///
/// # Safety
///
/// `name` and `service` NULL or NUL-terminated; `hints` NULL or readable;
/// `pai` writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getaddrinfo(
    name: *const u8,
    service: *const u8,
    hints: *const Addrinfo,
    pai: *mut *mut Addrinfo,
) -> i32 {
    if pai.is_null() {
        // glibc writes through it; here it is the caller's fault, reported.
        errno::set_errno(errno::EFAULT);
        return EAI_SYSTEM;
    }
    // SAFETY: the caller's strings.
    let mut name = (!name.is_null()).then(|| unsafe { crate::nss_files::c_bytes(name) });
    // SAFETY: as above.
    let mut serv = (!service.is_null()).then(|| unsafe { crate::nss_files::c_bytes(service) });
    if name == Some(b"*".as_slice()) {
        name = None;
    }
    if serv == Some(b"*".as_slice()) {
        serv = None;
    }
    if name.is_none() && serv.is_none() {
        return EAI_NONAME;
    }
    let mut req = if hints.is_null() {
        Req {
            flags: AI_V4MAPPED | AI_ADDRCONFIG,
            family: AF_UNSPEC,
            socktype: 0,
            protocol: 0,
        }
    } else {
        // SAFETY: readable by contract.
        let h = unsafe { &*hints };
        Req {
            flags: h.ai_flags,
            family: h.ai_family,
            socktype: h.ai_socktype,
            protocol: h.ai_protocol,
        }
    };
    let known = AI_PASSIVE
        | AI_CANONNAME
        | AI_NUMERICHOST
        | AI_ADDRCONFIG
        | AI_V4MAPPED
        | AI_IDN
        | AI_CANONIDN
        | DEPRECATED_AI_IDN
        | AI_NUMERICSERV
        | AI_ALL;
    if req.flags & !known != 0 {
        return EAI_BADFLAGS;
    }
    if req.flags & AI_CANONNAME != 0 && name.is_none() {
        return EAI_BADFLAGS;
    }
    if req.family != AF_UNSPEC && req.family != AF_INET && req.family != AF_INET6 {
        return EAI_FAMILY;
    }
    if req.flags & AI_ADDRCONFIG != 0 {
        let (seen_ipv4, seen_ipv6) = check_pf();
        if req.family == AF_UNSPEC && (seen_ipv4 || seen_ipv6) {
            if seen_ipv4 != seen_ipv6 {
                req.family = if seen_ipv4 { AF_INET } else { AF_INET6 };
            }
        } else if (req.family == AF_INET && !seen_ipv4) || (req.family == AF_INET6 && !seen_ipv6) {
            return EAI_NONAME;
        }
    }
    let service = match serv.filter(|s| !s.is_empty()) {
        Some(text) => {
            let num = match service_number(text) {
                Some(n) => n,
                None if req.flags & AI_NUMERICSERV != 0 => return EAI_NONAME,
                None => -1,
            };
            Some(Service { name: service, num })
        }
        None => None,
    };
    let mut head: *mut Addrinfo = core::ptr::null_mut();
    let naddrs = match gaih_inet(name, service.as_ref(), &req, &raw mut head) {
        Ok(n) => n,
        Err(e) => {
            // SAFETY: what was built, still linked and terminated.
            unsafe { freeaddrinfo(head) };
            return e;
        }
    };
    if naddrs > 1 {
        match with_tables(|t| sort(head, t)) {
            Ok(h) => head = h,
            Err(e) => {
                // SAFETY: as above.
                unsafe { freeaddrinfo(head) };
                return e;
            }
        }
    }
    if head.is_null() {
        return EAI_NONAME;
    }
    // SAFETY: writable by contract.
    unsafe { *pai = head };
    0
}

// ---------------------------------------------------------------------------
// getnameinfo
// ---------------------------------------------------------------------------

/// Return the numeric host.
pub const NI_NUMERICHOST: i32 = 0x01;
/// Return the numeric service.
pub const NI_NUMERICSERV: i32 = 0x02;
/// Only the host part of a name in this machine's own domain.
pub const NI_NOFQDN: i32 = 0x04;
/// A name, or `EAI_NONAME`.
pub const NI_NAMEREQD: i32 = 0x08;
/// The datagram (UDP) service.
pub const NI_DGRAM: i32 = 0x10;
/// Decode an internationalized name (glibc's).
pub const NI_IDN: i32 = 0x20;
/// `NI_IDN_ALLOW_UNASSIGNED` and `NI_IDN_USE_STD3_ASCII_RULES`: accepted and
/// ignored, as glibc accepts them.
const DEPRECATED_NI_IDN: i32 = 0xc0;
/// A numeric IPv6 scope, never an interface name: musl's flag, which
/// glibc does not have; a program built against musl's header passes it.
pub const NI_NUMERICSCOPE: i32 = 0x100;

/// Copy `s` and a NUL into `dst` of `len` bytes: `EAI_OVERFLOW` when it does
/// not fit (glibc's `checked_copy`).
fn copy_out(dst: *mut u8, len: usize, s: &[u8]) -> Result<(), i32> {
    if s.len() + 1 > len {
        return Err(crate::socket::EAI_OVERFLOW);
    }
    // SAFETY: `dst` holds `len` bytes, more than the text.
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), dst, s.len());
        dst.add(s.len()).write(0);
    }
    Ok(())
}

/// A decimal number's text.
fn decimal(n: u64, buf: &mut [u8; 20]) -> &[u8] {
    let mut i = buf.len();
    let mut n = n;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    &buf[i..]
}

crate::perprocess::process_global! {
    /// `nrl_domainname`'s answer: this machine's domain, found once.  The
    /// state (0 unknown, 1 known), the domain and its length.
    fn own_domain() -> (u8, [u8; 256], usize) = (0, [0; 256], 0);
}

/// `nrl_domainname`: this machine's domain -- after the first `.` of
/// `localhost`'s canonical name, else of the host name, else of the host
/// name's canonical name, else of 127.0.0.1's name.  Empty when none of
/// them has one.
fn domain() -> &'static [u8] {
    // SAFETY: the process's record (the thread's, on the host).
    let d = unsafe { &mut *own_domain() };
    if d.0 == 0 {
        let mut found: Option<([u8; 256], usize)> = None;
        // The part after the first dot of the first name that has one.
        let take = |found: &mut Option<([u8; 256], usize)>, name: &[u8]| {
            if found.is_none() {
                if let Some(i) = name.iter().position(|&b| b == b'.') {
                    let dom = &name[i + 1..];
                    let mut buf = [0u8; 256];
                    let n = dom.len().min(255);
                    buf[..n].copy_from_slice(&dom[..n]);
                    *found = Some((buf, n));
                }
            }
        };
        with_hostent_name(c"localhost".to_bytes_with_nul(), &mut |n| {
            take(&mut found, n)
        });
        let mut host = [0u8; 256];
        let hn = crate::unistd::gethostname(host.as_mut_ptr(), host.len() - 1) == 0;
        let hlen = host.iter().position(|&b| b == 0).unwrap_or(0);
        if hn {
            take(&mut found, &host[..hlen]);
            let mut named = [0u8; 257];
            named[..hlen].copy_from_slice(&host[..hlen]);
            with_hostent_name(&named[..=hlen], &mut |n| take(&mut found, n));
        }
        if found.is_none() {
            let lo = [127u8, 0, 0, 1];
            with_addr_name(&lo, &mut |n| take(&mut found, n));
        }
        if let Some((buf, n)) = found {
            d.1 = buf;
            d.2 = n;
        }
        d.0 = 1;
    }
    d.1.get(..d.2).unwrap_or(&[])
}

/// `gethostbyname_r(name)`'s canonical name, to `f`.
fn with_hostent_name(name_nul: &[u8], f: &mut dyn FnMut(&[u8])) {
    let mut buf = [0u8; 1024];
    let mut h = crate::socket::Hostent {
        h_name: core::ptr::null(),
        h_aliases: core::ptr::null(),
        h_addrtype: 0,
        h_length: 0,
        h_addr_list: core::ptr::null(),
    };
    let mut r: *const crate::socket::Hostent = core::ptr::null();
    let mut herr = 0;
    // SAFETY: a NUL-terminated name; outputs this function owns.
    let rc = unsafe {
        hosts::gethostbyname_r(
            name_nul.as_ptr(),
            &mut h,
            buf.as_mut_ptr(),
            buf.len(),
            &mut r,
            &mut herr,
        )
    };
    if rc == 0 && !r.is_null() {
        // SAFETY: the answer's name, in `buf`.
        f(unsafe { crate::nss_files::c_bytes(h.h_name) });
    }
}

/// `gethostbyaddr_r(addr)`'s name, to `f`.
fn with_addr_name(v4: &[u8; 4], f: &mut dyn FnMut(&[u8])) {
    let mut buf = [0u8; 1024];
    let mut h = crate::socket::Hostent {
        h_name: core::ptr::null(),
        h_aliases: core::ptr::null(),
        h_addrtype: 0,
        h_length: 0,
        h_addr_list: core::ptr::null(),
    };
    let mut r: *const crate::socket::Hostent = core::ptr::null();
    let mut herr = 0;
    // SAFETY: four readable bytes; outputs this function owns.
    let rc = unsafe {
        hosts::gethostbyaddr_r(
            v4.as_ptr(),
            4,
            AF_INET,
            &mut h,
            buf.as_mut_ptr(),
            buf.len(),
            &mut r,
            &mut herr,
        )
    };
    if rc == 0 && !r.is_null() {
        // SAFETY: the answer's name, in `buf`.
        f(unsafe { crate::nss_files::c_bytes(h.h_name) });
    }
}

/// `gni_host_inet_name`: the address's name into `host`.  `EAI_NONAME`
/// when it has none (the caller may fall back to the number).
fn host_name(
    addr: &[u8],
    family: i32,
    host: *mut u8,
    hostlen: usize,
    flags: i32,
) -> Result<(), i32> {
    let mut len = 1024usize;
    loop {
        let buf = crate::malloc::malloc(len);
        if buf.is_null() {
            return Err(EAI_MEMORY);
        }
        let mut h = crate::socket::Hostent {
            h_name: core::ptr::null(),
            h_aliases: core::ptr::null(),
            h_addrtype: 0,
            h_length: 0,
            h_addr_list: core::ptr::null(),
        };
        let mut r: *const crate::socket::Hostent = core::ptr::null();
        let mut herr = crate::socket::get_h_errno();
        // SAFETY: `addr` holds its family's bytes; outputs this function owns.
        let rc = unsafe {
            hosts::gethostbyaddr_r(
                addr.as_ptr(),
                addr.len() as u32,
                family,
                &mut h,
                buf,
                len,
                &mut r,
                &mut herr,
            )
        };
        if rc != 0 && herr == NETDB_INTERNAL && errno::get_errno() == errno::ERANGE {
            // SAFETY: this function's block.
            unsafe { crate::malloc::free(buf) };
            len *= 2;
            continue;
        }
        let out = if r.is_null() {
            crate::socket::set_h_errno(herr);
            if herr == NETDB_INTERNAL {
                Err(EAI_SYSTEM)
            } else if herr == TRY_AGAIN {
                Err(EAI_AGAIN)
            } else {
                Err(EAI_NONAME)
            }
        } else {
            // SAFETY: the answer's name, in `buf`.
            let mut name = unsafe { crate::nss_files::c_bytes(h.h_name) };
            if flags & NI_NOFQDN != 0 {
                let dom = domain();
                // `strstr (h_name, domain)`: the first place the domain
                // occurs, when a `.` precedes it.
                if !dom.is_empty() {
                    if let Some(at) = name.windows(dom.len()).position(|w| w == dom) {
                        if at > 0 && name[at - 1] == b'.' {
                            name = &name[..at - 1];
                        }
                    }
                }
            }
            // `NI_IDN`: no `libidn2` to decode with; the name is itself.
            copy_out(host, hostlen, name)
        };
        // SAFETY: this function's block; the name was copied out.
        unsafe { crate::malloc::free(buf) };
        return out;
    }
}

/// `gni_host_inet_numeric`: the address as text, an IPv6 scope after a `%`
/// -- the interface's name for a link-local address, unless
/// `NI_NUMERICSCOPE`.
fn host_numeric(
    addr: &[u8],
    family: i32,
    scope: u32,
    host: *mut u8,
    hostlen: usize,
    flags: i32,
) -> Result<(), i32> {
    let r = crate::inet::inet_ntop(family, addr.as_ptr(), host, hostlen as u32);
    if r.is_null() {
        return Err(crate::socket::EAI_OVERFLOW);
    }
    if family != AF_INET6 || scope == 0 {
        return Ok(());
    }
    // SAFETY: `inet_ntop` wrote a NUL-terminated address into `host`.
    let used = unsafe { crate::string::strlen(host) };
    let mut a = [0u8; 16];
    a.copy_from_slice(&addr[..16]);
    let mut text = [0u8; 40];
    let mut n = 0usize;
    text[0] = SCOPE_DELIMITER;
    n += 1;
    let mut named = false;
    if flags & NI_NUMERICSCOPE == 0 && (is_linklocal(&a) || is_mc_linklocal(&a)) {
        let mut ifname = [0u8; 16];
        // SAFETY: `ifname` holds IF_NAMESIZE bytes.
        if !unsafe { crate::socket::if_indextoname(scope, ifname.as_mut_ptr()) }.is_null() {
            let l = ifname.iter().position(|&b| b == 0).unwrap_or(ifname.len());
            text[n..n + l].copy_from_slice(&ifname[..l]);
            n += l;
            named = true;
        }
    }
    if !named {
        let mut num = [0u8; 20];
        let d = decimal(u64::from(scope), &mut num);
        text[n..n + d.len()].copy_from_slice(d);
        n += d.len();
    }
    // `snprintf` into what is left: a truncated scope is `EAI_OVERFLOW`.
    // SAFETY: `used < hostlen`, and `host` holds `hostlen` bytes.
    copy_out(unsafe { host.add(used) }, hostlen - used, &text[..n])
}

/// `gni_host`: the host half of `getnameinfo`.
///
/// # Safety
///
/// `sa` readable for `salen` bytes, which cover its family's address;
/// `host` writable for `hostlen` bytes.
unsafe fn gni_host(
    sa: *const Sockaddr,
    family: i32,
    host: *mut u8,
    hostlen: usize,
    flags: i32,
) -> Result<(), i32> {
    if family == crate::socket::AF_UNIX {
        if flags & NI_NUMERICHOST == 0 {
            // SAFETY: `Utsname` is byte arrays, for which zero is valid.
            let mut u: crate::utsname::Utsname = unsafe { core::mem::zeroed() };
            if crate::utsname::uname(&mut u) == 0 {
                let n = u
                    .nodename
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(u.nodename.len());
                return copy_out(host, hostlen, &u.nodename[..n]);
            }
        }
        if flags & NI_NAMEREQD != 0 {
            return Err(EAI_NONAME);
        }
        return copy_out(host, hostlen, b"localhost");
    }
    let v4: [u8; 4] = if family == AF_INET {
        // SAFETY: the caller's contract: a whole `sockaddr_in`.
        unsafe { (*sa.cast::<SockaddrIn>()).sin_addr.s_addr }.to_ne_bytes()
    } else {
        [0; 4]
    };
    let (addr, scope): (&[u8], u32) = if family == AF_INET6 {
        // SAFETY: the caller's contract: a whole `sockaddr_in6`.
        let s6 = unsafe { &*sa.cast::<SockaddrIn6>() };
        (&s6.sin6_addr.s6_addr, s6.sin6_scope_id)
    } else {
        (&v4, 0)
    };
    if flags & NI_NUMERICHOST == 0 {
        match host_name(addr, family, host, hostlen, flags) {
            Err(EAI_NONAME) => {}
            other => return other,
        }
    }
    if flags & NI_NAMEREQD != 0 {
        return Err(EAI_NONAME);
    }
    host_numeric(addr, family, scope, host, hostlen, flags)
}

/// `gni_serv`: the service half of `getnameinfo`.
///
/// # Safety
///
/// As [`gni_host`], for `serv` and `servlen`.
unsafe fn gni_serv(
    sa: *const Sockaddr,
    salen: usize,
    family: i32,
    serv: *mut u8,
    servlen: usize,
    flags: i32,
) -> Result<(), i32> {
    if family == crate::socket::AF_UNIX {
        // The path follows the family, to its NUL or the address's end.
        // SAFETY: `salen` bytes are readable, and at least the family's two.
        let path = unsafe { core::slice::from_raw_parts(sa.cast::<u8>().add(2), salen - 2) };
        let n = path.iter().position(|&b| b == 0).unwrap_or(path.len());
        return copy_out(serv, servlen, &path[..n]);
    }
    // The port is at the same offset in both forms.
    // SAFETY: the caller's contract.
    let port = unsafe { (*sa.cast::<SockaddrIn>()).sin_port };
    if flags & NI_NUMERICSERV == 0 {
        let proto: &[u8] = if flags & NI_DGRAM != 0 {
            b"udp\0"
        } else {
            b"tcp\0"
        };
        let mut s = crate::netdb::Servent {
            s_name: core::ptr::null(),
            s_aliases: core::ptr::null(),
            s_port: 0,
            s_proto: core::ptr::null(),
        };
        let mut result: *const crate::netdb::Servent = core::ptr::null();
        let mut len = 1024usize;
        loop {
            let buf = crate::malloc::malloc(len);
            if buf.is_null() {
                return Err(EAI_MEMORY);
            }
            // SAFETY: a NUL-terminated protocol; outputs and `len` bytes
            // this function owns.
            let rc = unsafe {
                crate::netdb::getservbyport_r(
                    i32::from(port),
                    proto.as_ptr(),
                    &mut s,
                    buf,
                    len,
                    &mut result,
                )
            };
            if rc == errno::ERANGE {
                // SAFETY: this function's block.
                unsafe { crate::malloc::free(buf) };
                len *= 2;
                continue;
            }
            let out = if rc == 0 && !result.is_null() {
                // SAFETY: the answer's name, in `buf`.
                Some(copy_out(serv, servlen, unsafe {
                    crate::nss_files::c_bytes(s.s_name)
                }))
            } else {
                None
            };
            // SAFETY: this function's block; the name was copied out.
            unsafe { crate::malloc::free(buf) };
            if let Some(o) = out {
                return o;
            }
            break;
        }
    }
    let mut num = [0u8; 20];
    copy_out(
        serv,
        servlen,
        decimal(u64::from(u16::from_be(port)), &mut num),
    )
}

/// Translate a socket address into a host and service name: glibc's
/// `getnameinfo`.  0, or an `EAI_*` code.
///
/// # Safety
///
/// `sa` NULL or readable for `salen` bytes; `host` NULL or writable for
/// `hostlen` bytes; `serv` NULL or writable for `servlen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getnameinfo(
    sa: *const Sockaddr,
    salen: SocklenT,
    host: *mut u8,
    hostlen: SocklenT,
    serv: *mut u8,
    servlen: SocklenT,
    flags: i32,
) -> i32 {
    let known = NI_NUMERICHOST
        | NI_NUMERICSERV
        | NI_NOFQDN
        | NI_NAMEREQD
        | NI_DGRAM
        | NI_IDN
        | DEPRECATED_NI_IDN
        | NI_NUMERICSCOPE;
    if flags & !known != 0 {
        return EAI_BADFLAGS;
    }
    let salen = salen as usize;
    if sa.is_null() || salen < size_of::<u16>() {
        return EAI_FAMILY;
    }
    if flags & NI_NAMEREQD != 0 && host.is_null() && serv.is_null() {
        return EAI_NONAME;
    }
    // SAFETY: at least the family is readable.
    let family = i32::from(unsafe { (*sa).sa_family });
    let need = match family {
        crate::socket::AF_UNIX => size_of::<u16>(),
        AF_INET => size_of::<SockaddrIn>(),
        AF_INET6 => size_of::<SockaddrIn6>(),
        _ => return EAI_FAMILY,
    };
    if salen < need {
        return EAI_FAMILY;
    }
    if !host.is_null() && hostlen > 0 {
        // SAFETY: the caller's contract, and `salen` covers the address.
        if let Err(e) = unsafe { gni_host(sa, family, host, hostlen as usize, flags) } {
            return e;
        }
    }
    if !serv.is_null() && servlen > 0 {
        // SAFETY: as above.
        if let Err(e) = unsafe { gni_serv(sa, salen, family, serv, servlen as usize, flags) } {
            return e;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nss_files::{Which, set_test_text};
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    fn arg(s: &str) -> Option<Vec<u8>> {
        match s {
            "NULL" => None,
            "EMPTY" => Some(std::vec![0]),
            _ => {
                let mut v = s.as_bytes().to_vec();
                v.push(0);
                Some(v)
            }
        }
    }

    fn ptr(a: Option<&Vec<u8>>) -> *const u8 {
        a.map_or(core::ptr::null(), |v| v.as_ptr())
    }

    fn strtol0(s: &str) -> i64 {
        let (neg, t) = s.strip_prefix('-').map_or((false, s), |t| (true, t));
        let v = if let Some(h) = t.strip_prefix("0x") {
            i64::from_str_radix(h, 16).unwrap()
        } else {
            t.parse().unwrap()
        };
        if neg { -v } else { v }
    }

    fn ntop(af: i32, p: *const u8) -> String {
        let mut out = [0u8; 64];
        let r = crate::inet::inet_ntop(af, p, out.as_mut_ptr(), 64);
        // SAFETY: `inet_ntop` wrote a NUL-terminated string.
        String::from_utf8_lossy(unsafe { crate::nss_files::c_bytes(r) }).into_owned()
    }

    /// `gai_oracle.c`'s `show`.
    fn show(rc: i32, res: *mut Addrinfo) -> String {
        if rc != 0 {
            return format!("rc={rc}");
        }
        let mut out = String::new();
        let mut a = res;
        while !a.is_null() {
            // SAFETY: a live entry of the answer.
            let ai = unsafe { &*a };
            let (t, port, scope) = if ai.ai_family == AF_INET {
                // SAFETY: an `AF_INET` entry's address is a `sockaddr_in`.
                let s = unsafe { &*ai.ai_addr.cast::<SockaddrIn>() };
                (
                    ntop(AF_INET, (&raw const s.sin_addr).cast()),
                    u16::from_be(s.sin_port),
                    0,
                )
            } else {
                // SAFETY: as above, `sockaddr_in6`.
                let s = unsafe { &*ai.ai_addr.cast::<SockaddrIn6>() };
                (
                    ntop(AF_INET6, s.sin6_addr.s6_addr.as_ptr()),
                    u16::from_be(s.sin6_port),
                    s.sin6_scope_id,
                )
            };
            out += &format!(
                "[f={:x} {} {} {} {t} {port}",
                ai.ai_flags, ai.ai_family, ai.ai_socktype, ai.ai_protocol
            );
            if scope != 0 {
                out += &format!(" %{scope}");
            }
            if !ai.ai_canonname.is_null() {
                // SAFETY: a NUL-terminated name.
                out += &format!(
                    " c={}",
                    String::from_utf8_lossy(unsafe { crate::nss_files::c_bytes(ai.ai_canonname) })
                );
            }
            out += "]";
            a = ai.ai_next;
        }
        out
    }

    /// What `gai_oracle.c` prints for `cmd`.
    fn run(cmd: &str) -> String {
        let a: Vec<&str> = cmd.split(' ').collect();
        // SAFETY (every call): NUL-terminated strings, and outputs this
        // function owns.
        unsafe {
            match a[0] {
                "gai" => {
                    let (n, s) = (arg(a[1]), arg(a[2]));
                    let mut res: *mut Addrinfo = core::ptr::null_mut();
                    let rc = if a[3] == "nohints" {
                        getaddrinfo(
                            ptr(n.as_ref()),
                            ptr(s.as_ref()),
                            core::ptr::null(),
                            &mut res,
                        )
                    } else {
                        let h = Addrinfo {
                            ai_flags: strtol0(a[3]) as i32,
                            ai_family: a[4].parse().unwrap(),
                            ai_socktype: a[5].parse().unwrap(),
                            ai_protocol: a[6].parse().unwrap(),
                            ai_addrlen: 0,
                            ai_addr: core::ptr::null_mut(),
                            ai_canonname: core::ptr::null_mut(),
                            ai_next: core::ptr::null_mut(),
                        };
                        getaddrinfo(ptr(n.as_ref()), ptr(s.as_ref()), &h, &mut res)
                    };
                    let line = show(rc, res);
                    if rc == 0 {
                        freeaddrinfo(res);
                    }
                    line
                }
                "gni" => {
                    let fam: i32 = a[1].parse().unwrap();
                    let mut ss = [0u8; 128];
                    let len = match fam {
                        AF_INET => {
                            let mut s = SockaddrIn {
                                sin_family: AF_INET as u16,
                                sin_port: a[3].parse::<u16>().unwrap().to_be(),
                                sin_addr: InAddr { s_addr: 0 },
                                sin_zero: [0; 8],
                            };
                            let t = arg(a[2]).unwrap();
                            crate::inet::inet_pton(
                                AF_INET,
                                t.as_ptr(),
                                (&raw mut s.sin_addr).cast(),
                            );
                            ss.as_mut_ptr().cast::<SockaddrIn>().write_unaligned(s);
                            size_of::<SockaddrIn>()
                        }
                        AF_INET6 => {
                            let mut s = SockaddrIn6 {
                                sin6_family: AF_INET6 as u16,
                                sin6_port: a[3].parse::<u16>().unwrap().to_be(),
                                sin6_flowinfo: 0,
                                sin6_addr: In6Addr { s6_addr: [0; 16] },
                                sin6_scope_id: a[4].parse().unwrap(),
                            };
                            let t = arg(a[2]).unwrap();
                            crate::inet::inet_pton(
                                AF_INET6,
                                t.as_ptr(),
                                s.sin6_addr.s6_addr.as_mut_ptr(),
                            );
                            ss.as_mut_ptr().cast::<SockaddrIn6>().write_unaligned(s);
                            size_of::<SockaddrIn6>()
                        }
                        1 => {
                            ss[0] = 1;
                            ss[2..2 + a[2].len()].copy_from_slice(a[2].as_bytes());
                            110
                        }
                        f => {
                            ss[..2].copy_from_slice(&(f as u16).to_ne_bytes());
                            16
                        }
                    };
                    let mut host = [b'#'; 1100];
                    let mut serv = [b'#'; 100];
                    let (hl, sl): (u32, u32) = (a[5].parse().unwrap(), a[6].parse().unwrap());
                    let rc = getnameinfo(
                        ss.as_ptr().cast(),
                        len as u32,
                        if hl != 0 {
                            host.as_mut_ptr()
                        } else {
                            core::ptr::null_mut()
                        },
                        hl,
                        if sl != 0 {
                            serv.as_mut_ptr()
                        } else {
                            core::ptr::null_mut()
                        },
                        sl,
                        strtol0(a[7]) as i32,
                    );
                    if rc != 0 {
                        return format!("rc={rc}");
                    }
                    let h = if hl != 0 {
                        String::from_utf8_lossy(crate::nss_files::c_bytes(host.as_ptr()))
                            .into_owned()
                    } else {
                        String::from("-")
                    };
                    let s = if sl != 0 {
                        String::from_utf8_lossy(crate::nss_files::c_bytes(serv.as_ptr()))
                            .into_owned()
                    } else {
                        String::from("-")
                    };
                    format!("host={h} serv={s}")
                }
                "err" => {
                    let mut out = String::new();
                    for c in -110..=2 {
                        out += &format!(
                            "{c}:{}|",
                            String::from_utf8_lossy(crate::nss_files::c_bytes(gai_strerror(c)))
                        );
                    }
                    out
                }
                other => panic!("unknown command {other}"),
            }
        }
    }

    /// The oracle's machine: one IPv4 address, 10.0.2.15/24, a gateway,
    /// no IPv6, and a DNS server that refuses every question.
    fn machine(gaiconf: &'static [u8]) {
        set_test_text(Which::Hosts, Some(HOSTS_FILE));
        set_test_text(Which::HostConf, Some(HOSTCONF_FILE));
        set_test_text(Which::Services, Some(SERVICES_FILE));
        set_test_text(Which::GaiConf, Some(gaiconf));
        hosts::reset_host_conf();
        reset_tables();
        hosts::set_test_resolver(hosts::TestResolver {
            names: &[],
            reverse: &[],
            otherwise: errno::ECONNREFUSED,
        });
        crate::socket::set_test_eth0(Some((
            u32::from_ne_bytes([10, 0, 2, 15]),
            u32::from_ne_bytes([255, 255, 255, 0]),
            u32::from_ne_bytes([10, 0, 2, 2]),
        )));
    }

    fn compare(gaiconf: &'static [u8], want: &str) {
        machine(gaiconf);
        let mut wrong = Vec::new();
        for (i, (cmd, w)) in COMMANDS.lines().zip(want.lines()).enumerate() {
            let got = run(cmd);
            if got != w {
                wrong.push(format!("{} `{cmd}`\n  glibc: {w}\n  ours:  {got}", i + 1));
            }
        }
        assert_eq!(COMMANDS.lines().count(), want.lines().count());
        crate::socket::set_test_eth0(None);
        assert!(
            wrong.is_empty(),
            "{} differ:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    #[test]
    fn every_answer_is_glibcs_with_its_own_tables() {
        compare(GAICONF_DEFAULT, GLIBC_DEFAULT);
    }

    #[test]
    fn every_answer_is_glibcs_with_a_gai_conf() {
        compare(GAICONF_CUSTOM, GLIBC_CUSTOM);
    }

    /// musl's `NI_NUMERICSCOPE`, which glibc refuses as an unknown flag: a
    /// program built against musl's header means it.
    #[test]
    fn ni_numericscope_is_musls_flag() {
        machine(GAICONF_DEFAULT);
        assert_eq!(
            run("gni 10 fe80::1 80 1 100 100 0x101"),
            "host=fe80::1%1 serv=http"
        );
        crate::socket::set_test_eth0(None);
    }

    /// An IPv4 answer mapped into an IPv6 question is unusable here -- there
    /// are no IPv6 sockets -- so it sorts by precedence with the native
    /// IPv6 ones, after them, where Linux's dual-stack sockets would make it
    /// usable and put it first.
    #[test]
    fn mapped_answers_sort_after_ipv6_here() {
        machine(GAICONF_DEFAULT);
        assert_eq!(
            run("gai mixed.example 80 0x18 10 1 0"),
            concat!(
                "[f=18 10 1 6 ::1 80][f=18 10 1 6 2001:db8::1 80]",
                "[f=18 10 1 6 ::ffff:10.0.2.99 80][f=18 10 1 6 ::ffff:8.8.8.8 80]",
                "[f=18 10 1 6 ::ffff:127.0.0.1 80][f=18 10 1 6 ::ffff:127.0.0.5 80]"
            )
        );
        crate::socket::set_test_eth0(None);
    }

    #[test]
    fn every_entry_is_its_own_block_and_all_are_freed() {
        use crate::malloc::live_allocations;
        machine(GAICONF_DEFAULT);
        let before = live_allocations::count();
        let h = Addrinfo {
            ai_flags: AI_CANONNAME,
            ai_family: 0,
            ai_socktype: 0,
            ai_protocol: 0,
            ai_addrlen: 0,
            ai_addr: core::ptr::null_mut(),
            ai_canonname: core::ptr::null_mut(),
            ai_next: core::ptr::null_mut(),
        };
        let mut a: *mut Addrinfo = core::ptr::null_mut();
        let mut b: *mut Addrinfo = core::ptr::null_mut();
        // SAFETY: NUL-terminated strings; outputs this test owns.
        unsafe {
            assert_eq!(
                getaddrinfo(
                    c"mixed.example".as_ptr().cast(),
                    c"80".as_ptr().cast(),
                    &h,
                    &mut a
                ),
                0
            );
            assert_eq!(
                getaddrinfo(
                    c"localhost".as_ptr().cast(),
                    c"22".as_ptr().cast(),
                    &h,
                    &mut b
                ),
                0
            );
            assert_ne!(a, b);
            // Freeing one list leaves the other intact.
            freeaddrinfo(a);
            assert_eq!(
                String::from_utf8_lossy(crate::nss_files::c_bytes((*b).ai_canonname)),
                "localhost"
            );
            freeaddrinfo(b);
            freeaddrinfo(core::ptr::null_mut());
        }
        crate::socket::set_test_eth0(None);
        hosts::reset_host_conf();
        assert_eq!(
            live_allocations::count(),
            before,
            "an entry or a name leaked"
        );
    }

    #[test]
    fn a_null_result_pointer_is_efault() {
        errno::set_errno(0);
        // SAFETY: NULL is the input under test.
        let rc = unsafe {
            getaddrinfo(
                c"1.2.3.4".as_ptr().cast(),
                core::ptr::null(),
                core::ptr::null(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!((rc, errno::get_errno()), (EAI_SYSTEM, errno::EFAULT));
    }

    // Generated by dlm/oracle/gai_harness.py: the files, the commands, and what
    // glibc 2.39 printed for them (gai_oracle.c), on one IPv4 address, 10.0.2.15/24.
    const HOSTS_FILE: &[u8] = b"127.0.0.1 localhost\n::1 localhost ip6-localhost\n10.0.2.99 near.example near\n8.8.8.8 far.example far\n10.0.2.99 mixed.example\n8.8.8.8 mixed.example\n::1 mixed.example\n127.0.0.5 mixed.example\n2001:db8::1 mixed.example\nfe80::1 linklocal.example\n169.254.1.1 ll4.example\n8.8.4.4 mixedll.example\n169.254.1.1 mixedll.example\n10.0.2.50 mixedll.example\n";
    const HOSTCONF_FILE: &[u8] = b"multi on\n";
    const SERVICES_FILE: &[u8] = b"http 80/tcp www\nhttp 80/udp\nssh 22/tcp\ndomain 53/tcp\ndomain 53/udp\ndccpsvc 1234/dccp\nsctpsvc 5000/sctp\nrawsvc 7/raw\nudponly 999/udp\n";
    const GAICONF_DEFAULT: &[u8] = b"";
    const GAICONF_CUSTOM: &[u8] = b"# a custom table\nlabel ::1/128 0\nlabel ::/0 1\nprecedence ::ffff:0:0/96 100\nprecedence ::1/128 50\nscopev4 ::ffff:169.254.0.0/112 2\nscopev4 ::ffff:10.0.0.0/104 5\nscopev4 8.8.0.0/16 7\nbogus line here\nprecedence ::/0 x\n";
    const COMMANDS: &str = "\
gai localhost NULL 0 0 0 0\n\
gai localhost http 0 0 0 0\n\
gai localhost http nohints\n\
gai localhost 80 0 0 1 0\n\
gai localhost 80 0 0 2 0\n\
gai localhost 80 0 0 3 0\n\
gai localhost NULL 0 0 3 0\n\
gai localhost NULL 0 0 3 99\n\
gai localhost 80 0 0 0 6\n\
gai localhost 80 0 0 0 17\n\
gai localhost 80 0 0 0 132\n\
gai localhost 80 0 0 5 0\n\
gai localhost 80 0 0 6 0\n\
gai localhost 80 0 0 1 17\n\
gai localhost 80 0 0 7 0\n\
gai localhost 80 0 0 0 99\n\
gai localhost NULL 0 0 0 99\n\
gai localhost ssh 0 0 0 0\n\
gai localhost domain 0 0 0 0\n\
gai localhost udponly 0 0 1 0\n\
gai localhost dccpsvc 0 0 0 0\n\
gai localhost sctpsvc 0 0 0 0\n\
gai localhost rawsvc 0 0 0 0\n\
gai localhost nosuch 0 0 0 0\n\
gai localhost nosuch 0x400 0 0 0\n\
gai localhost +80 0 0 1 0\n\
gai localhost -1 0 0 1 0\n\
gai localhost 70000 0 0 1 0\n\
gai localhost 4294967296 0 0 1 0\n\
gai localhost 2147483648 0 0 1 0\n\
gai localhost 99999999999999999999 0 0 1 0\n\
gai localhost EMPTY 0 0 1 0\n\
gai * 80 0 0 1 0\n\
gai * * 0 0 1 0\n\
gai NULL NULL 0 0 0 0\n\
gai NULL 80 1 0 1 0\n\
gai NULL 80 0 10 1 0\n\
gai NULL 80 1 2 1 0\n\
gai NULL 80 2 0 1 0\n\
gai localhost 80 0x8000 0 1 0\n\
gai localhost 80 0 99 1 0\n\
gai 1.2.3.4 80 0 0 1 0\n\
gai 1.2.3.4 80 0 10 1 0\n\
gai 1.2.3.4 80 8 10 1 0\n\
gai 127.1 80 0 0 1 0\n\
gai 0x7f000001 80 0 0 1 0\n\
gai ::1 80 0 0 1 0\n\
gai ::1 80 0 2 1 0\n\
gai ::ffff:1.2.3.4 80 0 2 1 0\n\
gai fe80::1%1 80 0 0 1 0\n\
gai fe80::1%lo 80 0 0 1 0\n\
gai fe80::1%bogus 80 0 0 1 0\n\
gai 2001:db8::1%5 80 0 0 1 0\n\
gai 1.2.3.4 80 2 0 1 0\n\
gai ::1 80 2 0 1 0\n\
gai near 80 2 0 1 0\n\
gai near 80 2 2 1 0\n\
gai near.example 80 0 0 1 0\n\
gai near 80 4 0 1 0\n\
gai nosuchhost 80 0 0 1 0\n\
gai nosuchhost 80 0 2 1 0\n\
gai nosuchhost 80 0 10 1 0\n\
gai mixed.example 80 0 0 1 0\n\
gai mixed.example 80 2 0 1 0\n\
gai mixed.example 80 0x20 0 1 0\n\
gai mixed.example 80 0 10 1 0\n\
gai mixed.example 80 8 10 1 0\n\
gai mixed.example 80 0 2 1 0\n\
gai mixedll.example 80 0 0 1 0\n\
gai ll4.example 80 0 0 1 0\n\
gai linklocal.example 80 0 0 1 0\n\
gai mixed.example NULL nohints\n\
gai localhost 80 0x40 0 1 0\n\
gai localhost 80 0x300 0 1 0\n\
gni 2 127.0.0.1 80 0 100 100 0\n\
gni 2 127.0.0.1 80 0 100 100 1\n\
gni 2 127.0.0.1 80 0 100 100 2\n\
gni 2 10.0.2.99 53 0 100 100 16\n\
gni 2 1.1.1.1 80 0 100 100 0\n\
gni 2 1.1.1.1 80 0 100 100 1\n\
gni 2 1.1.1.1 80 0 100 100 8\n\
gni 2 127.0.0.1 80 0 5 100 0\n\
gni 2 127.0.0.1 80 0 100 3 0\n\
gni 2 127.0.0.1 12345 0 100 100 0\n\
gni 2 127.0.0.1 12345 0 100 3 0\n\
gni 10 ::1 80 0 100 100 0\n\
gni 10 ::ffff:127.0.0.1 80 0 100 100 0\n\
gni 10 fe80::1 80 1 100 100 1\n\
gni 10 fe80::1 80 7 100 100 1\n\
gni 10 2001:db8::1 80 5 100 100 1\n\
gni 10 fe80::1 80 1 12 100 1\n\
gni 1 /tmp/sock 0 0 100 100 1\n\
gni 1 /tmp/sock 0 0 100 3 1\n\
gni 99 x 0 0 100 100 0\n\
gni 2 1.2.3.4 80 0 100 100 0x400\n\
gni 2 1.2.3.4 80 0 0 0 8\n\
gni 2 127.0.0.1 80 0 0 100 0\n\
gni 2 127.0.0.1 80 0 100 0 0\n\
err\n\
";
    const GLIBC_DEFAULT: &str = "\
[f=0 2 1 6 127.0.0.1 0][f=0 2 2 17 127.0.0.1 0][f=0 2 3 0 127.0.0.1 0][f=0 10 1 6 ::1 0][f=0 10 2 17 ::1 0][f=0 10 3 0 ::1 0]\n\
[f=0 2 1 6 127.0.0.1 80][f=0 2 2 17 127.0.0.1 80][f=0 10 1 6 ::1 80][f=0 10 2 17 ::1 80]\n\
[f=28 2 1 6 127.0.0.1 80][f=28 2 2 17 127.0.0.1 80][f=28 2 1 6 127.0.0.1 80][f=28 2 2 17 127.0.0.1 80]\n\
[f=0 2 1 6 127.0.0.1 80][f=0 10 1 6 ::1 80]\n\
[f=0 2 2 17 127.0.0.1 80][f=0 10 2 17 ::1 80]\n\
rc=-8\n\
[f=0 2 3 0 127.0.0.1 0][f=0 10 3 0 ::1 0]\n\
[f=0 2 3 99 127.0.0.1 0][f=0 10 3 99 ::1 0]\n\
[f=0 2 1 6 127.0.0.1 80][f=0 10 1 6 ::1 80]\n\
[f=0 2 2 17 127.0.0.1 80][f=0 10 2 17 ::1 80]\n\
[f=0 2 1 132 127.0.0.1 80][f=0 10 1 132 ::1 80]\n\
[f=0 2 5 132 127.0.0.1 80][f=0 10 5 132 ::1 80]\n\
[f=0 2 6 33 127.0.0.1 80][f=0 10 6 33 ::1 80]\n\
rc=-7\n\
rc=-7\n\
rc=-8\n\
[f=0 2 3 99 127.0.0.1 0][f=0 10 3 99 ::1 0]\n\
[f=0 2 1 6 127.0.0.1 22][f=0 10 1 6 ::1 22]\n\
[f=0 2 1 6 127.0.0.1 53][f=0 2 2 17 127.0.0.1 53][f=0 10 1 6 ::1 53][f=0 10 2 17 ::1 53]\n\
rc=-8\n\
[f=0 2 6 33 127.0.0.1 1234][f=0 10 6 33 ::1 1234]\n\
[f=0 2 1 132 127.0.0.1 5000][f=0 2 5 132 127.0.0.1 5000][f=0 10 1 132 ::1 5000][f=0 10 5 132 ::1 5000]\n\
rc=-8\n\
rc=-8\n\
rc=-2\n\
[f=0 2 1 6 127.0.0.1 80][f=0 10 1 6 ::1 80]\n\
rc=-8\n\
[f=0 2 1 6 127.0.0.1 4464][f=0 10 1 6 ::1 4464]\n\
[f=0 2 1 6 127.0.0.1 0][f=0 10 1 6 ::1 0]\n\
rc=-8\n\
rc=-8\n\
[f=0 2 1 6 127.0.0.1 0][f=0 10 1 6 ::1 0]\n\
[f=0 2 1 6 127.0.0.1 80][f=0 10 1 6 ::1 80]\n\
rc=-2\n\
rc=-2\n\
[f=1 2 1 6 0.0.0.0 80][f=1 10 1 6 :: 80]\n\
[f=0 10 1 6 ::1 80]\n\
[f=1 2 1 6 0.0.0.0 80]\n\
rc=-1\n\
rc=-1\n\
rc=-6\n\
[f=0 2 1 6 1.2.3.4 80]\n\
rc=-9\n\
[f=8 10 1 6 ::ffff:1.2.3.4 80]\n\
[f=0 2 1 6 127.0.0.1 80]\n\
[f=0 2 1 6 127.0.0.1 80]\n\
[f=0 10 1 6 ::1 80]\n\
rc=-9\n\
[f=0 2 1 6 1.2.3.4 80]\n\
[f=0 10 1 6 fe80::1 80 %1]\n\
[f=0 10 1 6 fe80::1 80 %1]\n\
rc=-2\n\
[f=0 10 1 6 2001:db8::1 80 %5]\n\
[f=2 2 1 6 1.2.3.4 80 c=1.2.3.4]\n\
[f=2 10 1 6 ::1 80 c=::1]\n\
[f=2 2 1 6 10.0.2.99 80 c=near.example]\n\
[f=2 2 1 6 10.0.2.99 80 c=near.example]\n\
[f=0 2 1 6 10.0.2.99 80]\n\
rc=-2\n\
rc=-3\n\
rc=-3\n\
rc=-3\n\
[f=0 2 1 6 127.0.0.5 80][f=0 2 1 6 10.0.2.99 80][f=0 2 1 6 8.8.8.8 80][f=0 10 1 6 ::1 80][f=0 10 1 6 2001:db8::1 80]\n\
[f=2 2 1 6 127.0.0.5 80 c=mixed.example][f=2 2 1 6 10.0.2.99 80][f=2 2 1 6 8.8.8.8 80][f=2 10 1 6 ::1 80][f=2 10 1 6 2001:db8::1 80]\n\
[f=20 2 1 6 127.0.0.1 80][f=20 2 1 6 127.0.0.5 80][f=20 2 1 6 10.0.2.99 80][f=20 2 1 6 8.8.8.8 80]\n\
[f=0 10 1 6 ::1 80][f=0 10 1 6 2001:db8::1 80]\n\
[f=8 10 1 6 ::1 80][f=8 10 1 6 2001:db8::1 80]\n\
[f=0 2 1 6 127.0.0.1 80][f=0 2 1 6 127.0.0.5 80][f=0 2 1 6 10.0.2.99 80][f=0 2 1 6 8.8.8.8 80]\n\
[f=0 2 1 6 8.8.4.4 80][f=0 2 1 6 10.0.2.50 80][f=0 2 1 6 169.254.1.1 80]\n\
[f=0 2 1 6 169.254.1.1 80]\n\
[f=0 10 1 6 fe80::1 80]\n\
[f=28 2 1 6 127.0.0.1 0][f=28 2 2 17 127.0.0.1 0][f=28 2 3 0 127.0.0.1 0][f=28 2 1 6 127.0.0.5 0][f=28 2 2 17 127.0.0.5 0][f=28 2 3 0 127.0.0.5 0][f=28 2 1 6 10.0.2.99 0][f=28 2 2 17 10.0.2.99 0][f=28 2 3 0 10.0.2.99 0][f=28 2 1 6 8.8.8.8 0][f=28 2 2 17 8.8.8.8 0][f=28 2 3 0 8.8.8.8 0]\n\
[f=40 2 1 6 127.0.0.1 80][f=40 10 1 6 ::1 80]\n\
[f=300 2 1 6 127.0.0.1 80][f=300 10 1 6 ::1 80]\n\
host=localhost serv=http\n\
host=127.0.0.1 serv=http\n\
host=localhost serv=80\n\
host=near.example serv=domain\n\
rc=-3\n\
host=1.1.1.1 serv=http\n\
rc=-3\n\
rc=-12\n\
rc=-12\n\
host=localhost serv=12345\n\
rc=-12\n\
host=localhost serv=http\n\
host=localhost serv=http\n\
host=fe80::1%lo serv=http\n\
host=fe80::1%7 serv=http\n\
host=2001:db8::1%5 serv=http\n\
host=fe80::1%lo serv=http\n\
host=localhost serv=/tmp/sock\n\
rc=-12\n\
rc=-6\n\
rc=-1\n\
rc=-2\n\
host=- serv=http\n\
host=localhost serv=-\n\
-110:Unknown error|-109:Unknown error|-108:Unknown error|-107:Unknown error|-106:Unknown error|-105:Parameter string not correctly encoded|-104:Interrupted by a signal|-103:All requests done|-102:Request not canceled|-101:Request canceled|-100:Processing request in progress|-99:Unknown error|-98:Unknown error|-97:Unknown error|-96:Unknown error|-95:Unknown error|-94:Unknown error|-93:Unknown error|-92:Unknown error|-91:Unknown error|-90:Unknown error|-89:Unknown error|-88:Unknown error|-87:Unknown error|-86:Unknown error|-85:Unknown error|-84:Unknown error|-83:Unknown error|-82:Unknown error|-81:Unknown error|-80:Unknown error|-79:Unknown error|-78:Unknown error|-77:Unknown error|-76:Unknown error|-75:Unknown error|-74:Unknown error|-73:Unknown error|-72:Unknown error|-71:Unknown error|-70:Unknown error|-69:Unknown error|-68:Unknown error|-67:Unknown error|-66:Unknown error|-65:Unknown error|-64:Unknown error|-63:Unknown error|-62:Unknown error|-61:Unknown error|-60:Unknown error|-59:Unknown error|-58:Unknown error|-57:Unknown error|-56:Unknown error|-55:Unknown error|-54:Unknown error|-53:Unknown error|-52:Unknown error|-51:Unknown error|-50:Unknown error|-49:Unknown error|-48:Unknown error|-47:Unknown error|-46:Unknown error|-45:Unknown error|-44:Unknown error|-43:Unknown error|-42:Unknown error|-41:Unknown error|-40:Unknown error|-39:Unknown error|-38:Unknown error|-37:Unknown error|-36:Unknown error|-35:Unknown error|-34:Unknown error|-33:Unknown error|-32:Unknown error|-31:Unknown error|-30:Unknown error|-29:Unknown error|-28:Unknown error|-27:Unknown error|-26:Unknown error|-25:Unknown error|-24:Unknown error|-23:Unknown error|-22:Unknown error|-21:Unknown error|-20:Unknown error|-19:Unknown error|-18:Unknown error|-17:Unknown error|-16:Unknown error|-15:Unknown error|-14:Unknown error|-13:Unknown error|-12:Result too large for supplied buffer|-11:System error|-10:Memory allocation failure|-9:Address family for hostname not supported|-8:Servname not supported for ai_socktype|-7:ai_socktype not supported|-6:ai_family not supported|-5:No address associated with hostname|-4:Non-recoverable failure in name resolution|-3:Temporary failure in name resolution|-2:Name or service not known|-1:Bad value for ai_flags|0:Success|1:Unknown error|2:Unknown error|\n\
";
    const GLIBC_CUSTOM: &str = "\
[f=0 2 1 6 127.0.0.1 0][f=0 2 2 17 127.0.0.1 0][f=0 2 3 0 127.0.0.1 0][f=0 10 1 6 ::1 0][f=0 10 2 17 ::1 0][f=0 10 3 0 ::1 0]\n\
[f=0 2 1 6 127.0.0.1 80][f=0 2 2 17 127.0.0.1 80][f=0 10 1 6 ::1 80][f=0 10 2 17 ::1 80]\n\
[f=28 2 1 6 127.0.0.1 80][f=28 2 2 17 127.0.0.1 80][f=28 2 1 6 127.0.0.1 80][f=28 2 2 17 127.0.0.1 80]\n\
[f=0 2 1 6 127.0.0.1 80][f=0 10 1 6 ::1 80]\n\
[f=0 2 2 17 127.0.0.1 80][f=0 10 2 17 ::1 80]\n\
rc=-8\n\
[f=0 2 3 0 127.0.0.1 0][f=0 10 3 0 ::1 0]\n\
[f=0 2 3 99 127.0.0.1 0][f=0 10 3 99 ::1 0]\n\
[f=0 2 1 6 127.0.0.1 80][f=0 10 1 6 ::1 80]\n\
[f=0 2 2 17 127.0.0.1 80][f=0 10 2 17 ::1 80]\n\
[f=0 2 1 132 127.0.0.1 80][f=0 10 1 132 ::1 80]\n\
[f=0 2 5 132 127.0.0.1 80][f=0 10 5 132 ::1 80]\n\
[f=0 2 6 33 127.0.0.1 80][f=0 10 6 33 ::1 80]\n\
rc=-7\n\
rc=-7\n\
rc=-8\n\
[f=0 2 3 99 127.0.0.1 0][f=0 10 3 99 ::1 0]\n\
[f=0 2 1 6 127.0.0.1 22][f=0 10 1 6 ::1 22]\n\
[f=0 2 1 6 127.0.0.1 53][f=0 2 2 17 127.0.0.1 53][f=0 10 1 6 ::1 53][f=0 10 2 17 ::1 53]\n\
rc=-8\n\
[f=0 2 6 33 127.0.0.1 1234][f=0 10 6 33 ::1 1234]\n\
[f=0 2 1 132 127.0.0.1 5000][f=0 2 5 132 127.0.0.1 5000][f=0 10 1 132 ::1 5000][f=0 10 5 132 ::1 5000]\n\
rc=-8\n\
rc=-8\n\
rc=-2\n\
[f=0 2 1 6 127.0.0.1 80][f=0 10 1 6 ::1 80]\n\
rc=-8\n\
[f=0 2 1 6 127.0.0.1 4464][f=0 10 1 6 ::1 4464]\n\
[f=0 2 1 6 127.0.0.1 0][f=0 10 1 6 ::1 0]\n\
rc=-8\n\
rc=-8\n\
[f=0 2 1 6 127.0.0.1 0][f=0 10 1 6 ::1 0]\n\
[f=0 2 1 6 127.0.0.1 80][f=0 10 1 6 ::1 80]\n\
rc=-2\n\
rc=-2\n\
[f=1 2 1 6 0.0.0.0 80][f=1 10 1 6 :: 80]\n\
[f=0 10 1 6 ::1 80]\n\
[f=1 2 1 6 0.0.0.0 80]\n\
rc=-1\n\
rc=-1\n\
rc=-6\n\
[f=0 2 1 6 1.2.3.4 80]\n\
rc=-9\n\
[f=8 10 1 6 ::ffff:1.2.3.4 80]\n\
[f=0 2 1 6 127.0.0.1 80]\n\
[f=0 2 1 6 127.0.0.1 80]\n\
[f=0 10 1 6 ::1 80]\n\
rc=-9\n\
[f=0 2 1 6 1.2.3.4 80]\n\
[f=0 10 1 6 fe80::1 80 %1]\n\
[f=0 10 1 6 fe80::1 80 %1]\n\
rc=-2\n\
[f=0 10 1 6 2001:db8::1 80 %5]\n\
[f=2 2 1 6 1.2.3.4 80 c=1.2.3.4]\n\
[f=2 10 1 6 ::1 80 c=::1]\n\
[f=2 2 1 6 10.0.2.99 80 c=near.example]\n\
[f=2 2 1 6 10.0.2.99 80 c=near.example]\n\
[f=0 2 1 6 10.0.2.99 80]\n\
rc=-2\n\
rc=-3\n\
rc=-3\n\
rc=-3\n\
[f=0 2 1 6 10.0.2.99 80][f=0 2 1 6 127.0.0.5 80][f=0 2 1 6 8.8.8.8 80][f=0 10 1 6 ::1 80][f=0 10 1 6 2001:db8::1 80]\n\
[f=2 2 1 6 10.0.2.99 80 c=mixed.example][f=2 2 1 6 127.0.0.5 80][f=2 2 1 6 8.8.8.8 80][f=2 10 1 6 ::1 80][f=2 10 1 6 2001:db8::1 80]\n\
[f=20 2 1 6 10.0.2.99 80][f=20 2 1 6 127.0.0.1 80][f=20 2 1 6 127.0.0.5 80][f=20 2 1 6 8.8.8.8 80]\n\
[f=0 10 1 6 ::1 80][f=0 10 1 6 2001:db8::1 80]\n\
[f=8 10 1 6 ::1 80][f=8 10 1 6 2001:db8::1 80]\n\
[f=0 2 1 6 10.0.2.99 80][f=0 2 1 6 127.0.0.1 80][f=0 2 1 6 127.0.0.5 80][f=0 2 1 6 8.8.8.8 80]\n\
[f=0 2 1 6 10.0.2.50 80][f=0 2 1 6 169.254.1.1 80][f=0 2 1 6 8.8.4.4 80]\n\
[f=0 2 1 6 169.254.1.1 80]\n\
[f=0 10 1 6 fe80::1 80]\n\
[f=28 2 1 6 10.0.2.99 0][f=28 2 2 17 10.0.2.99 0][f=28 2 3 0 10.0.2.99 0][f=28 2 1 6 127.0.0.1 0][f=28 2 2 17 127.0.0.1 0][f=28 2 3 0 127.0.0.1 0][f=28 2 1 6 127.0.0.5 0][f=28 2 2 17 127.0.0.5 0][f=28 2 3 0 127.0.0.5 0][f=28 2 1 6 8.8.8.8 0][f=28 2 2 17 8.8.8.8 0][f=28 2 3 0 8.8.8.8 0]\n\
[f=40 2 1 6 127.0.0.1 80][f=40 10 1 6 ::1 80]\n\
[f=300 2 1 6 127.0.0.1 80][f=300 10 1 6 ::1 80]\n\
host=localhost serv=http\n\
host=127.0.0.1 serv=http\n\
host=localhost serv=80\n\
host=near.example serv=domain\n\
rc=-3\n\
host=1.1.1.1 serv=http\n\
rc=-3\n\
rc=-12\n\
rc=-12\n\
host=localhost serv=12345\n\
rc=-12\n\
host=localhost serv=http\n\
host=localhost serv=http\n\
host=fe80::1%lo serv=http\n\
host=fe80::1%7 serv=http\n\
host=2001:db8::1%5 serv=http\n\
host=fe80::1%lo serv=http\n\
host=localhost serv=/tmp/sock\n\
rc=-12\n\
rc=-6\n\
rc=-1\n\
rc=-2\n\
host=- serv=http\n\
host=localhost serv=-\n\
-110:Unknown error|-109:Unknown error|-108:Unknown error|-107:Unknown error|-106:Unknown error|-105:Parameter string not correctly encoded|-104:Interrupted by a signal|-103:All requests done|-102:Request not canceled|-101:Request canceled|-100:Processing request in progress|-99:Unknown error|-98:Unknown error|-97:Unknown error|-96:Unknown error|-95:Unknown error|-94:Unknown error|-93:Unknown error|-92:Unknown error|-91:Unknown error|-90:Unknown error|-89:Unknown error|-88:Unknown error|-87:Unknown error|-86:Unknown error|-85:Unknown error|-84:Unknown error|-83:Unknown error|-82:Unknown error|-81:Unknown error|-80:Unknown error|-79:Unknown error|-78:Unknown error|-77:Unknown error|-76:Unknown error|-75:Unknown error|-74:Unknown error|-73:Unknown error|-72:Unknown error|-71:Unknown error|-70:Unknown error|-69:Unknown error|-68:Unknown error|-67:Unknown error|-66:Unknown error|-65:Unknown error|-64:Unknown error|-63:Unknown error|-62:Unknown error|-61:Unknown error|-60:Unknown error|-59:Unknown error|-58:Unknown error|-57:Unknown error|-56:Unknown error|-55:Unknown error|-54:Unknown error|-53:Unknown error|-52:Unknown error|-51:Unknown error|-50:Unknown error|-49:Unknown error|-48:Unknown error|-47:Unknown error|-46:Unknown error|-45:Unknown error|-44:Unknown error|-43:Unknown error|-42:Unknown error|-41:Unknown error|-40:Unknown error|-39:Unknown error|-38:Unknown error|-37:Unknown error|-36:Unknown error|-35:Unknown error|-34:Unknown error|-33:Unknown error|-32:Unknown error|-31:Unknown error|-30:Unknown error|-29:Unknown error|-28:Unknown error|-27:Unknown error|-26:Unknown error|-25:Unknown error|-24:Unknown error|-23:Unknown error|-22:Unknown error|-21:Unknown error|-20:Unknown error|-19:Unknown error|-18:Unknown error|-17:Unknown error|-16:Unknown error|-15:Unknown error|-14:Unknown error|-13:Unknown error|-12:Result too large for supplied buffer|-11:System error|-10:Memory allocation failure|-9:Address family for hostname not supported|-8:Servname not supported for ai_socktype|-7:ai_socktype not supported|-6:ai_family not supported|-5:No address associated with hostname|-4:Non-recoverable failure in name resolution|-3:Temporary failure in name resolution|-2:Name or service not known|-1:Bad value for ai_flags|0:Success|1:Unknown error|2:Unknown error|\n\
";
}
