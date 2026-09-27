// Every index here is into a fixed-size array -- four bytes of an IPv4
// address, sixteen of an IPv6 one, eight 16-bit words, six of an Ethernet
// address, a formatting buffer sized for the longest text -- by a counter the
// loop bounds below that size, and every sum is of small bounded values.
// Clippy cannot see the bounds.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
//! `<arpa/inet.h>` and `<netinet/ether.h>`: Internet and Ethernet addresses
//! turned from text into binary and back, as glibc 2.40 turns them.
//!
//! Each function is a port of glibc's, line for line where the line decides
//! an answer, because the edges are where programs disagree:
//!
//! - **`inet_aton` and `inet_addr`** read the old BSD forms as well as the
//!   dotted quad: `127.1` is 127.0.0.1 (the last part fills the bytes that
//!   remain), each part is a C number -- `0x7f` hexadecimal, `0177` octal --
//!   and the address may be followed by white space and anything after it
//!   (`resolv/inet_addr.c`, `inet_aton_end`).  [`aton_exact`] is glibc's
//!   `__inet_aton_exact`, which is the same but must reach the end: what
//!   `getaddrinfo` and `gethostbyname` accept as a numeric host.
//! - **`inet_pton`** is the strict form (`resolv/inet_pton.c`): four decimal
//!   parts for IPv4 with no leading zeros, and for IPv6 at most four hex
//!   digits a group, one `::`, and a dotted quad only as the last 32 bits.
//! - **`inet_ntop`** writes IPv6 as glibc does (`resolv/inet_ntop.c`): the
//!   first longest run of two or more zero groups becomes `::`, and an
//!   address whose first 96 bits are zero, or `::ffff:0:0/96`, ends in a
//!   dotted quad.
//! - **`inet_network`, `inet_makeaddr`, `inet_lnaof`, `inet_netof`** are the
//!   classful (pre-CIDR) helpers, kept because old programs still call them.
//! - **`ether_aton`, `ether_ntoa`** and their `_r` forms, and
//!   **`ether_line`**, read and write `xx:xx:xx:xx:xx:xx`.
//!
//! The functions that hand back a pointer to library storage (`inet_ntoa`,
//! `ether_aton`, `ether_ntoa`) use the calling thread's block
//! ([`crate::perthread`]), so two threads never overwrite each other's
//! answer; glibc shares one buffer between them.
//!
//! A NULL string is refused (0, `INADDR_NONE`, NULL) where glibc would read
//! through it -- the same answer as for text that is not an address.

use crate::errno;
use crate::socket::{AF_INET, AF_INET6, InAddr};

/// `INADDR_NONE`: `inet_addr`'s and `inet_network`'s failure.
pub const INADDR_NONE: u32 = 0xFFFF_FFFF;

/// `INET_ADDRSTRLEN`: room for the longest IPv4 text, NUL included.
pub const INET_ADDRSTRLEN: usize = 16;
/// `INET6_ADDRSTRLEN`: room for the longest IPv6 text, NUL included.
pub const INET6_ADDRSTRLEN: usize = 46;

/// C's `isspace` in the C locale.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The value of a hexadecimal digit (glibc's `hex_digit_value`).
fn hex_value(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some(u32::from(b - b'0')),
        b'a'..=b'f' => Some(u32::from(b - b'a' + 10)),
        b'A'..=b'F' => Some(u32::from(b - b'A' + 10)),
        _ => None,
    }
}

/// A NUL-terminated string's bytes, or `None` for NULL.
///
/// # Safety
///
/// `s` is NULL or a NUL-terminated string.
unsafe fn c_str<'a>(s: *const u8) -> Option<&'a [u8]> {
    if s.is_null() {
        return None;
    }
    // SAFETY: the caller's contract.
    Some(unsafe { core::slice::from_raw_parts(s, crate::string::strlen(s)) })
}

// ---------------------------------------------------------------------------
// inet_aton, inet_addr
// ---------------------------------------------------------------------------

/// `strtoul (s, &end, 0)` for text that starts with a digit: `0x` then hex,
/// `0` then octal, decimal otherwise.  The value -- anything past 32 bits
/// reported as `u64::MAX`, which every caller refuses -- and the bytes used.
fn strtoul_base0(s: &[u8]) -> (u64, usize) {
    let (base, start) = match s {
        [b'0', b'x' | b'X', d, ..] if d.is_ascii_hexdigit() => (16u64, 2usize),
        [b'0', ..] => (8, 1),
        _ => (10, 0),
    };
    let mut value: u64 = 0;
    let mut i = start;
    while let Some(&b) = s.get(i) {
        let Some(d) = hex_value(b).map(u64::from) else {
            break;
        };
        if d >= base {
            break;
        }
        value = value.saturating_mul(base).saturating_add(d);
        i += 1;
    }
    (value, i)
}

/// glibc's `inet_aton_end`: the address, network byte order, and where the
/// text that made it ends.
fn aton_end(s: &[u8]) -> Option<([u8; 4], usize)> {
    // The most the last part may hold, by how many parts came before it:
    // `a` 32 bits, `a.b` 24, `a.b.c` 16, `a.b.c.d` 8.
    const MAX: [u32; 4] = [0xffff_ffff, 0x00ff_ffff, 0xffff, 0xff];
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut bytes = [0u8; 4];
    let mut parts = 0usize;
    let mut i = 0usize;
    let val = loop {
        if !at(i).is_ascii_digit() {
            return None;
        }
        let (ul, used) = strtoul_base0(s.get(i..).unwrap_or(&[]));
        let val = u32::try_from(ul).ok()?;
        i += used;
        if at(i) == b'.' {
            if parts > 2 || val > 0xff {
                return None;
            }
            bytes[parts] = val as u8;
            parts += 1;
            i += 1;
        } else {
            break val;
        }
    };
    let c = at(i);
    if c != 0 && !is_space(c) {
        return None;
    }
    if val > MAX[parts] {
        return None;
    }
    let word = u32::from_be_bytes(bytes) | val;
    Some((word.to_be_bytes(), i))
}

/// glibc's `__inet_aton_exact`: `inet_aton`, but the whole text must be the
/// address -- what a numeric host is to `getaddrinfo` and `gethostbyname`.
// Their ports, in the next commit, are its callers; until then only the
// tests call it.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn aton_exact(s: &[u8]) -> Option<[u8; 4]> {
    match aton_end(s) {
        Some((addr, end)) if end == s.len() => Some(addr),
        _ => None,
    }
}

/// Convert an IPv4 address in any of the BSD forms to binary.
///
/// 1 and the address in `*inp` (unless `inp` is NULL, which glibc allows:
/// the text is only checked), or 0 for text that is not an address.
///
/// # Safety
///
/// `cp` is NULL or NUL-terminated; `inp` is NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet_aton(cp: *const u8, inp: *mut InAddr) -> i32 {
    // SAFETY: the caller's contract.
    let Some(s) = (unsafe { c_str(cp) }) else {
        return 0;
    };
    let Some((addr, _)) = aton_end(s) else {
        return 0;
    };
    if !inp.is_null() {
        // SAFETY: `inp` is writable by contract.
        unsafe {
            (*inp).s_addr = u32::from_ne_bytes(addr);
        }
    }
    1
}

/// Convert an IPv4 address in any of the BSD forms to binary: the address
/// in network byte order, or `INADDR_NONE` -- which is also
/// `255.255.255.255`, the reason `inet_aton` exists.
///
/// # Safety
///
/// `cp` is NULL or NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet_addr(cp: *const u8) -> u32 {
    // SAFETY: the caller's contract.
    match unsafe { c_str(cp) }.and_then(aton_end) {
        Some((addr, _)) => u32::from_ne_bytes(addr),
        None => INADDR_NONE,
    }
}

// ---------------------------------------------------------------------------
// inet_pton
// ---------------------------------------------------------------------------

/// glibc's `inet_pton4`: exactly four decimal parts of at most 255, none
/// with a leading zero.
pub(crate) fn pton4(src: &[u8]) -> Option<[u8; 4]> {
    let mut tmp = [0u8; 4];
    let mut tp = 0usize;
    let mut saw_digit = false;
    let mut octets = 0u32;
    for &ch in src {
        if ch.is_ascii_digit() {
            let new = u32::from(tmp[tp]) * 10 + u32::from(ch - b'0');
            if saw_digit && tmp[tp] == 0 {
                return None;
            }
            if new > 255 {
                return None;
            }
            tmp[tp] = new as u8;
            if !saw_digit {
                octets += 1;
                if octets > 4 {
                    return None;
                }
                saw_digit = true;
            }
        } else if ch == b'.' && saw_digit {
            if octets == 4 {
                return None;
            }
            tp += 1;
            tmp[tp] = 0;
            saw_digit = false;
        } else {
            return None;
        }
    }
    (octets >= 4).then_some(tmp)
}

/// glibc's `inet_pton6`.
pub(crate) fn pton6(src: &[u8]) -> Option<[u8; 16]> {
    let mut tmp = [0u8; 16];
    let mut tp = 0usize;
    let mut colonp: Option<usize> = None;
    let mut i = 0usize;
    // Leading `::` needs its own test: a lone leading `:` is no address.
    match src {
        [] => return None,
        [b':', rest @ ..] => {
            if rest.first() != Some(&b':') {
                return None;
            }
            i = 1;
        }
        _ => {}
    }
    let mut curtok = i;
    let mut xdigits_seen = 0u32;
    let mut val = 0u32;
    while let Some(&ch) = src.get(i) {
        i += 1;
        if let Some(d) = hex_value(ch) {
            if xdigits_seen == 4 {
                return None;
            }
            val = (val << 4) | d;
            if val > 0xffff {
                return None;
            }
            xdigits_seen += 1;
            continue;
        }
        if ch == b':' {
            curtok = i;
            if xdigits_seen == 0 {
                if colonp.is_some() {
                    return None;
                }
                colonp = Some(tp);
                continue;
            } else if i == src.len() {
                return None;
            }
            if tp + 2 > 16 {
                return None;
            }
            tmp[tp] = (val >> 8) as u8;
            tmp[tp + 1] = val as u8;
            tp += 2;
            xdigits_seen = 0;
            val = 0;
            continue;
        }
        if ch == b'.' && tp + 4 <= 16 {
            if let Some(v4) = pton4(src.get(curtok..).unwrap_or(&[])) {
                tmp[tp..tp + 4].copy_from_slice(&v4);
                tp += 4;
                xdigits_seen = 0;
                break;
            }
        }
        return None;
    }
    if xdigits_seen > 0 {
        if tp + 2 > 16 {
            return None;
        }
        tmp[tp] = (val >> 8) as u8;
        tmp[tp + 1] = val as u8;
        tp += 2;
    }
    if let Some(cp) = colonp {
        // `::` would stand for no groups at all.
        if tp == 16 {
            return None;
        }
        let n = tp - cp;
        tmp.copy_within(cp..tp, 16 - n);
        tmp[cp..16 - n].fill(0);
        tp = 16;
    }
    (tp == 16).then_some(tmp)
}

/// Convert an address from text to binary: 1 and the address in `dst` (4
/// bytes for `AF_INET`, 16 for `AF_INET6`), 0 for text that is not one, -1
/// with `EAFNOSUPPORT` for another family.  `dst` is untouched unless 1.
///
/// # Safety
///
/// `src` is NULL or NUL-terminated; `dst` is NULL or writable for the
/// family's size.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet_pton(af: i32, src: *const u8, dst: *mut u8) -> i32 {
    if af != AF_INET && af != AF_INET6 {
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }
    // SAFETY: the caller's contract.
    let Some(s) = (unsafe { c_str(src) }) else {
        return 0;
    };
    if dst.is_null() {
        return 0;
    }
    // SAFETY (both arms): `dst` is writable for the family's size.
    if af == AF_INET {
        match pton4(s) {
            Some(a) => unsafe { core::ptr::copy_nonoverlapping(a.as_ptr(), dst, 4) },
            None => return 0,
        }
    } else {
        match pton6(s) {
            Some(a) => unsafe { core::ptr::copy_nonoverlapping(a.as_ptr(), dst, 16) },
            None => return 0,
        }
    }
    1
}

// ---------------------------------------------------------------------------
// inet_ntop, inet_ntoa
// ---------------------------------------------------------------------------

/// Text being built in a fixed buffer.
struct Text<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> Text<N> {
    const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    fn push(&mut self, b: u8) {
        if let Some(slot) = self.buf.get_mut(self.len) {
            *slot = b;
            self.len += 1;
        }
    }

    fn decimal(&mut self, v: u8) {
        if v >= 100 {
            self.push(b'0' + v / 100);
        }
        if v >= 10 {
            self.push(b'0' + v / 10 % 10);
        }
        self.push(b'0' + v % 10);
    }

    /// `%x`: lowercase, no leading zeros.
    fn hex(&mut self, v: u32) {
        let mut started = false;
        for shift in (0..8).rev() {
            let d = (v >> (shift * 4)) & 0xf;
            if d != 0 || started || shift == 0 {
                self.push(b"0123456789abcdef"[d as usize]);
                started = true;
            }
        }
    }

    fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

/// glibc's `inet_ntop4` format: `%u.%u.%u.%u`.
fn ntop4(a: &[u8; 4]) -> Text<INET_ADDRSTRLEN> {
    let mut t = Text::new();
    for (i, &b) in a.iter().enumerate() {
        if i > 0 {
            t.push(b'.');
        }
        t.decimal(b);
    }
    t
}

/// glibc's `inet_ntop6` format.
fn ntop6(a: &[u8; 16]) -> Text<INET6_ADDRSTRLEN> {
    let mut words = [0u32; 8];
    for (i, w) in words.iter_mut().enumerate() {
        *w = u32::from(a[i * 2]) << 8 | u32::from(a[i * 2 + 1]);
    }
    // The first longest run of zero words; runs of one are not shortened.
    let mut best: Option<(usize, usize)> = None;
    let mut cur: Option<(usize, usize)> = None;
    for (i, &w) in words.iter().enumerate() {
        if w == 0 {
            cur = Some(cur.map_or((i, 1), |(b, l)| (b, l + 1)));
        } else if let Some(c) = cur.take() {
            if best.is_none_or(|b| c.1 > b.1) {
                best = Some(c);
            }
        }
    }
    if let Some(c) = cur {
        if best.is_none_or(|b| c.1 > b.1) {
            best = Some(c);
        }
    }
    let best = best.filter(|&(_, len)| len >= 2);

    let mut t = Text::new();
    let mut i = 0usize;
    while i < 8 {
        if let Some((base, len)) = best {
            if i >= base && i < base + len {
                if i == base {
                    t.push(b':');
                }
                i += 1;
                continue;
            }
        }
        if i != 0 {
            t.push(b':');
        }
        // An IPv4-compatible (`::a.b.c.d`) or IPv4-mapped (`::ffff:a.b.c.d`)
        // address ends in its dotted quad.
        if i == 6
            && best.is_some_and(|(base, len)| {
                base == 0 && (len == 6 || (len == 5 && words[5] == 0xffff))
            })
        {
            let v4 = ntop4(&[a[12], a[13], a[14], a[15]]);
            for &b in v4.bytes() {
                t.push(b);
            }
            return t;
        }
        t.hex(words[i]);
        i += 1;
    }
    if best.is_some_and(|(base, len)| base + len == 8) {
        t.push(b':');
    }
    t
}

/// Convert an address from binary to text: `dst`, or NULL with `errno` --
/// `EAFNOSUPPORT` for another family, `ENOSPC` when `size` cannot hold the
/// text and its NUL.
///
/// A NULL pointer is `EFAULT` where glibc would fault, in glibc's order: it
/// reads `src` first and reaches `dst` only in its final copy, after the
/// `ENOSPC` test.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inet_ntop(af: i32, src: *const u8, dst: *mut u8, size: u32) -> *const u8 {
    if af != AF_INET && af != AF_INET6 {
        errno::set_errno(errno::EAFNOSUPPORT);
        return core::ptr::null();
    }
    if src.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null();
    }
    if af == AF_INET {
        let mut a = [0u8; 4];
        // SAFETY: `src` is non-null and holds four bytes (caller's contract).
        unsafe { core::ptr::copy_nonoverlapping(src, a.as_mut_ptr(), 4) };
        deliver(ntop4(&a).bytes(), dst, size)
    } else {
        let mut a = [0u8; 16];
        // SAFETY: `src` is non-null and holds sixteen bytes.
        unsafe { core::ptr::copy_nonoverlapping(src, a.as_mut_ptr(), 16) };
        deliver(ntop6(&a).bytes(), dst, size)
    }
}

/// `inet_ntop`'s last step: the text and its NUL into `dst` if `size`
/// holds them.
fn deliver(text: &[u8], dst: *mut u8, size: u32) -> *const u8 {
    if text.len() >= size as usize {
        errno::set_errno(errno::ENOSPC);
        return core::ptr::null();
    }
    if dst.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null();
    }
    // SAFETY: `dst` holds `size` bytes, more than the text.
    unsafe {
        core::ptr::copy_nonoverlapping(text.as_ptr(), dst, text.len());
        dst.add(text.len()).write(0);
    }
    dst.cast_const()
}

/// Convert an IPv4 address to dotted-quad text, in the calling thread's
/// buffer, which its next call overwrites.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inet_ntoa(addr: InAddr) -> *const u8 {
    let text = ntop4(&addr.s_addr.to_ne_bytes());
    // SAFETY: the calling thread's block, touched by no other thread; the
    // buffer holds `INET_ADDRSTRLEN` bytes, and the text is at most 15.
    unsafe {
        let buf = &mut (*crate::perthread::current()).inet_ntoa;
        buf.fill(0);
        buf[..text.len].copy_from_slice(text.bytes());
        buf.as_ptr()
    }
}

// ---------------------------------------------------------------------------
// The classful helpers
// ---------------------------------------------------------------------------

/// Convert a network number in dotted form to binary, host byte order:
/// glibc's `inet_network`, `INADDR_NONE` for text that is not one.  Parts
/// are joined byte by byte (`127.1` is `0x7f01`), each at most 255, each
/// decimal, `0` octal or `0x` hexadecimal; white space may follow.
///
/// # Safety
///
/// `cp` is NULL or NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet_network(cp: *const u8) -> u32 {
    // SAFETY: the caller's contract.
    let Some(s) = (unsafe { c_str(cp) }) else {
        return INADDR_NONE;
    };
    network(s)
}

/// The body of [`inet_network`], over the string's bytes.
pub(crate) fn network(s: &[u8]) -> u32 {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut parts = [0u32; 4];
    let mut n = 0usize;
    let mut i = 0usize;
    loop {
        let mut val: u32 = 0;
        let mut base: u32 = 10;
        let mut digit = false;
        if at(i) == b'0' {
            digit = true;
            base = 8;
            i += 1;
        }
        if matches!(at(i), b'x' | b'X') {
            digit = false;
            base = 16;
            i += 1;
        }
        loop {
            let c = at(i);
            if c.is_ascii_digit() {
                if base == 8 && (c == b'8' || c == b'9') {
                    return INADDR_NONE;
                }
                // glibc's `uint32_t` arithmetic: it wraps.
                val = val.wrapping_mul(base).wrapping_add(u32::from(c - b'0'));
                i += 1;
                digit = true;
                continue;
            }
            if base == 16 && c.is_ascii_hexdigit() {
                val = (val << 4).wrapping_add(u32::from(c.to_ascii_lowercase() + 10 - b'a'));
                i += 1;
                digit = true;
                continue;
            }
            break;
        }
        if !digit || n >= 4 || val > 0xff {
            return INADDR_NONE;
        }
        if at(i) == b'.' {
            parts[n] = val;
            n += 1;
            i += 1;
            continue;
        }
        while is_space(at(i)) {
            i += 1;
        }
        if at(i) != 0 {
            return INADDR_NONE;
        }
        parts[n] = val;
        n += 1;
        break;
    }
    parts[..n]
        .iter()
        .fold(0u32, |acc, &p| (acc << 8) | (p & 0xff))
}

// The classful split, from `<netinet/in.h>`.
const IN_CLASSA_NET: u32 = 0xff00_0000;
const IN_CLASSA_NSHIFT: u32 = 24;
const IN_CLASSA_HOST: u32 = 0x00ff_ffff;
const IN_CLASSB_NET: u32 = 0xffff_0000;
const IN_CLASSB_NSHIFT: u32 = 16;
const IN_CLASSB_HOST: u32 = 0x0000_ffff;
const IN_CLASSC_NET: u32 = 0xffff_ff00;
const IN_CLASSC_NSHIFT: u32 = 8;
const IN_CLASSC_HOST: u32 = 0x0000_00ff;

fn in_class_a(i: u32) -> bool {
    i & 0x8000_0000 == 0
}

fn in_class_b(i: u32) -> bool {
    i & 0xc000_0000 == 0x8000_0000
}

/// Make an address from a network number and a host number, by the class
/// the network number's size implies.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inet_makeaddr(net: u32, host: u32) -> InAddr {
    let a = if net < 128 {
        (net << IN_CLASSA_NSHIFT) | (host & IN_CLASSA_HOST)
    } else if net < 65536 {
        (net << IN_CLASSB_NSHIFT) | (host & IN_CLASSB_HOST)
    } else if net < 16_777_216 {
        (net << IN_CLASSC_NSHIFT) | (host & IN_CLASSC_HOST)
    } else {
        net | host
    };
    InAddr { s_addr: a.to_be() }
}

/// The host part of an address, by its class.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inet_lnaof(addr: InAddr) -> u32 {
    let i = u32::from_be(addr.s_addr);
    if in_class_a(i) {
        i & IN_CLASSA_HOST
    } else if in_class_b(i) {
        i & IN_CLASSB_HOST
    } else {
        i & IN_CLASSC_HOST
    }
}

/// The network part of an address, by its class.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inet_netof(addr: InAddr) -> u32 {
    let i = u32::from_be(addr.s_addr);
    if in_class_a(i) {
        (i & IN_CLASSA_NET) >> IN_CLASSA_NSHIFT
    } else if in_class_b(i) {
        (i & IN_CLASSB_NET) >> IN_CLASSB_NSHIFT
    } else {
        (i & IN_CLASSC_NET) >> IN_CLASSC_NSHIFT
    }
}

// ---------------------------------------------------------------------------
// Ethernet addresses
// ---------------------------------------------------------------------------

/// `struct ether_addr`: six bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EtherAddr {
    /// The address, first byte first.
    pub ether_addr_octet: [u8; 6],
}

impl EtherAddr {
    /// All zero: the per-thread buffer's initial state.
    pub const ZERO: Self = Self {
        ether_addr_octet: [0; 6],
    };
}

/// glibc's `ether_aton_r` loop, which `ether_line` repeats: six groups of
/// one or two hex digits separated by `:`.  After the sixth group's first
/// digit, anything but the end or white space is taken as a second digit --
/// and refused unless it is one.  Each byte is stored as soon as it is
/// read, so a failure leaves the earlier ones written, as in glibc.  The
/// index just past the address, or `None`.
///
/// `line` is `ether_line`'s variant, which steps over the character after
/// the sixth group only when it is not the string's end.
fn ether_parse(s: &[u8], out: &mut [u8; 6], line: bool) -> Option<usize> {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut i = 0usize;
    for (cnt, slot) in out.iter_mut().enumerate() {
        let ch = at(i).to_ascii_lowercase();
        i += 1;
        let mut number = hex_value(ch)?;
        let ch = at(i).to_ascii_lowercase();
        let mut next = ch;
        if (cnt < 5 && ch != b':') || (cnt == 5 && ch != 0 && !is_space(ch)) {
            i += 1;
            number = (number << 4) + hex_value(ch)?;
            next = at(i);
            if cnt < 5 && next != b':' {
                return None;
            }
        }
        *slot = number as u8;
        if !line || next != 0 {
            i += 1;
        }
    }
    Some(i)
}

/// Convert `xx:xx:xx:xx:xx:xx` to an Ethernet address in `addr`: `addr`, or
/// NULL.
///
/// # Safety
///
/// `asc` is NULL or NUL-terminated; `addr` is NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ether_aton_r(asc: *const u8, addr: *mut EtherAddr) -> *mut EtherAddr {
    // SAFETY: the caller's contract.
    let Some(s) = (unsafe { c_str(asc) }) else {
        return core::ptr::null_mut();
    };
    if addr.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: `addr` is writable by contract.
    let out = unsafe { &mut (*addr).ether_addr_octet };
    match ether_parse(s, out, false) {
        Some(_) => addr,
        None => core::ptr::null_mut(),
    }
}

/// [`ether_aton_r`] into the calling thread's buffer.
///
/// # Safety
///
/// `asc` is NULL or NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ether_aton(asc: *const u8) -> *mut EtherAddr {
    // SAFETY: the calling thread's block, touched by no other thread.
    let slot = unsafe { &raw mut (*crate::perthread::current()).ether_aton };
    // SAFETY: `slot` is writable; `asc` per the caller's contract.
    unsafe { ether_aton_r(asc, slot) }
}

/// Write an Ethernet address as `%x:%x:%x:%x:%x:%x` into `buf`, which must
/// hold 18 bytes: `buf`.
///
/// # Safety
///
/// `addr` is readable and `buf` writable for 18 bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ether_ntoa_r(addr: *const EtherAddr, buf: *mut u8) -> *mut u8 {
    if addr.is_null() || buf.is_null() {
        return core::ptr::null_mut();
    }
    let mut t: Text<18> = Text::new();
    // SAFETY: `addr` is readable by contract.
    let octets = unsafe { (*addr).ether_addr_octet };
    for (i, &b) in octets.iter().enumerate() {
        if i > 0 {
            t.push(b':');
        }
        t.hex(u32::from(b));
    }
    // SAFETY: `buf` holds 18 bytes, and the text is at most 17.
    unsafe {
        core::ptr::copy_nonoverlapping(t.buf.as_ptr(), buf, t.len);
        buf.add(t.len).write(0);
    }
    buf
}

/// [`ether_ntoa_r`] into the calling thread's buffer.
///
/// # Safety
///
/// `addr` is readable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ether_ntoa(addr: *const EtherAddr) -> *mut u8 {
    // SAFETY: the calling thread's block, touched by no other thread.
    let buf = unsafe { (&raw mut (*crate::perthread::current()).ether_ntoa).cast::<u8>() };
    // SAFETY: the buffer holds 18 bytes; `addr` per the caller's contract.
    unsafe { ether_ntoa_r(addr, buf) }
}

/// Read an `/etc/ethers` line -- an address, white space, a host name --
/// into `addr` and `hostname`: 0, or -1 for a line that is not one.  The
/// name runs to white space, `#` or the end, and is copied unbounded, as
/// glibc copies it: `hostname` must be as long as the line.
///
/// # Safety
///
/// `line` is NULL or NUL-terminated; `addr` writable; `hostname` writable
/// for the line's length plus one.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ether_line(
    line: *const u8,
    addr: *mut EtherAddr,
    hostname: *mut u8,
) -> i32 {
    // SAFETY: the caller's contract.
    let Some(s) = (unsafe { c_str(line) }) else {
        return -1;
    };
    if addr.is_null() || hostname.is_null() {
        return -1;
    }
    // SAFETY: `addr` is writable by contract.
    let out = unsafe { &mut (*addr).ether_addr_octet };
    let Some(mut i) = ether_parse(s, out, true) else {
        return -1;
    };
    while s.get(i).is_some_and(|&b| is_space(b)) {
        i += 1;
    }
    let name_start = i;
    while s.get(i).is_some_and(|&b| b != b'#' && !is_space(b)) {
        i += 1;
    }
    if i == name_start {
        return -1;
    }
    let name = &s[name_start..i];
    // SAFETY: `hostname` holds the line's length plus one by contract, and
    // the name is part of the line.
    unsafe {
        core::ptr::copy_nonoverlapping(name.as_ptr(), hostname, name.len());
        hostname.add(name.len()).write(0);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    fn unhex(h: &str) -> Vec<u8> {
        (0..h.len() / 2)
            .map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    fn hex(b: &[u8]) -> String {
        use core::fmt::Write as _;
        b.iter().fold(String::new(), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
    }

    /// NUL-terminated copy of `b` (an embedded NUL ends it, as in C).
    fn cstring(b: &[u8]) -> Vec<u8> {
        let mut v = b.to_vec();
        v.push(0);
        v
    }

    fn text(p: *const u8) -> String {
        // SAFETY: the functions under test return NUL-terminated strings.
        String::from_utf8(unsafe { c_str(p) }.unwrap().to_vec()).unwrap()
    }

    /// What `addr_oracle.c` prints for one input, computed with this crate's
    /// functions.
    fn ours(kind: &str, arg: &str) -> String {
        match kind {
            "s" => {
                let s = cstring(&unhex(arg));
                let p = s.as_ptr();
                let mut a = InAddr {
                    s_addr: u32::from_ne_bytes([0xaa; 4]),
                };
                // SAFETY: `p` is NUL-terminated; the outputs are writable.
                let aton = unsafe { inet_aton(p, &mut a) };
                let addr = unsafe { inet_addr(p) };
                let mut p4 = [0xaau8; 4];
                let r4 = unsafe { inet_pton(AF_INET, p, p4.as_mut_ptr()) };
                let mut p6 = [0xaau8; 16];
                let r6 = unsafe { inet_pton(AF_INET6, p, p6.as_mut_ptr()) };
                let net = unsafe { inet_network(p) };
                let mut e = EtherAddr {
                    ether_addr_octet: [0xaa; 6],
                };
                let ep = unsafe { ether_aton_r(p, &mut e) };
                format!(
                    "aton={aton}:{} addr={} pton4={r4}:{} pton6={r6}:{} network={net:08x} ether={}:{}",
                    hex(&a.s_addr.to_ne_bytes()),
                    hex(&addr.to_ne_bytes()),
                    hex(&p4),
                    hex(&p6),
                    i32::from(!ep.is_null()),
                    hex(&e.ether_addr_octet)
                )
            }
            "a6" | "a4" => {
                let af = if kind == "a6" { AF_INET6 } else { AF_INET };
                let src = unhex(arg);
                let mut out = [0u8; 64];
                let r = inet_ntop(af, src.as_ptr(), out.as_mut_ptr(), 64);
                let mut line = format!(
                    "ntop={}",
                    if r.is_null() {
                        String::from("NULL")
                    } else {
                        text(r)
                    }
                );
                if !r.is_null() {
                    let need = text(r).len() + 1;
                    errno::set_errno(0);
                    let r2 = inet_ntop(af, src.as_ptr(), out.as_mut_ptr(), (need - 1) as u32);
                    line += &format!(
                        " short={}/{}",
                        if r2.is_null() { "NULL" } else { "ok" },
                        errno::get_errno()
                    );
                }
                if af == AF_INET {
                    let ia = InAddr {
                        s_addr: u32::from_ne_bytes(src[..4].try_into().unwrap()),
                    };
                    line += &format!(
                        " ntoa={} lnaof={:08x} netof={:08x}",
                        text(inet_ntoa(ia)),
                        inet_lnaof(ia),
                        inet_netof(ia)
                    );
                }
                line
            }
            "mk" => {
                let mut it = arg.split(' ');
                let net = u32::from_str_radix(it.next().unwrap(), 16).unwrap();
                let host = u32::from_str_radix(it.next().unwrap(), 16).unwrap();
                format!(
                    "makeaddr={}",
                    hex(&inet_makeaddr(net, host).s_addr.to_ne_bytes())
                )
            }
            "e" => {
                let e = EtherAddr {
                    ether_addr_octet: unhex(arg).try_into().unwrap(),
                };
                let mut out = [0u8; 18];
                // SAFETY: `out` holds 18 bytes.
                let r = unsafe { ether_ntoa_r(&e, out.as_mut_ptr()) };
                format!("ntoa={}", text(r))
            }
            "el" => {
                let s = cstring(&unhex(arg));
                let mut e = EtherAddr {
                    ether_addr_octet: [0xaa; 6],
                };
                let mut host = std::vec![0u8; s.len() + 2];
                host[0] = b'-';
                // SAFETY: `host` is as long as the line plus one.
                let r = unsafe { ether_line(s.as_ptr(), &mut e, host.as_mut_ptr()) };
                let h = host.iter().position(|&b| b == 0).unwrap();
                format!("line={r}:{}:{}", hex(&e.ether_addr_octet), hex(&host[..h]))
            }
            _ => panic!("unknown kind {kind}"),
        }
    }

    #[test]
    fn every_answer_is_glibcs() {
        let mut wrong = Vec::new();
        for (input, want) in GLIBC {
            let (kind, arg) = input.split_once(' ').unwrap_or((input, ""));
            let got = ours(kind, arg);
            if got != *want {
                wrong.push(format!("{input}\n  glibc: {want}\n  ours:  {got}"));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} of {}:\n{}",
            wrong.len(),
            GLIBC.len(),
            wrong.join("\n")
        );
    }

    /// glibc 2.39's answers, from `addr_oracle.c` run under WSL: the
    /// input (`s` text, `a6`/`a4` address bytes, `mk` net host, `e`
    /// Ethernet bytes -- all hex) and the line it printed.
    const GLIBC: &[(&str, &str)] = &[
        (
            "s 312e322e332e34",
            "aton=1:01020304 addr=01020304 pton4=1:01020304 pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=01020304 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 302e302e302e30",
            "aton=1:00000000 addr=00000000 pton4=1:00000000 pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=00000000 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3235352e3235352e3235352e323535",
            "aton=1:ffffffff addr=ffffffff pton4=1:ffffffff pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3235362e312e312e31",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e33",
            "aton=1:01020003 addr=01020003 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=00010203 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e32",
            "aton=1:01000002 addr=01000002 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=00000102 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 31",
            "aton=1:00000001 addr=00000001 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=00000001 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3132372e31",
            "aton=1:7f000001 addr=7f000001 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=00007f01 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 31302e302e302e3120",
            "aton=1:0a000001 addr=0a000001 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=0a000001 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 31302e302e302e31206a756e6b",
            "aton=1:0a000001 addr=0a000001 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 31302e302e302e310978",
            "aton=1:0a000001 addr=0a000001 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 31302e302e302e3178",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 307837662e31",
            "aton=1:7f000001 addr=7f000001 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=00007f01 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 30783766303030303031",
            "aton=1:7f000001 addr=7f000001 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 30583746303030303031",
            "aton=1:7f000001 addr=7f000001 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 303137373030303030303031",
            "aton=1:7f000001 addr=7f000001 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 303137372e302e302e31",
            "aton=1:7f000001 addr=7f000001 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=7f000001 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 30312e30322e30332e3034",
            "aton=1:01020304 addr=01020304 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=01020304 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 30382e312e312e31",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3039",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e342e35",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e2e322e33",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 2e312e322e33",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s ",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 20312e322e332e34",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 2b312e322e332e34",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 2d31",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 34323934393637323935",
            "aton=1:ffffffff addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 34323934393637323936",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=00000000 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e3136373737323135",
            "aton=1:01ffffff addr=01ffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e3136373737323136",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e3635353335",
            "aton=1:0102ffff addr=0102ffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e3635353336",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3078",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 30782e312e322e33",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e307834",
            "aton=1:01020304 addr=01020304 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=01020304 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e3034",
            "aton=1:01020304 addr=01020304 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=01020304 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e340a",
            "aton=1:01020304 addr=01020304 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=01020304 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e340b",
            "aton=1:01020304 addr=01020304 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=01020304 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e3480",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 393939393939393939393939393939393939393939",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 30786666666666666666",
            "aton=1:ffffffff addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3078313030303030303030",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=00000000 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e3078666666666665",
            "aton=1:01fffffe addr=01fffffe pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3030",
            "aton=1:00000000 addr=00000000 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=00000000 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 30",
            "aton=1:00000000 addr=00000000 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=00000000 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 316532",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3078312e3078322e3078332e307834",
            "aton=1:01020304 addr=01020304 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=01020304 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e340d",
            "aton=1:01020304 addr=01020304 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=01020304 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e323536",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e2d31",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e2b31",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 307837662e3078302e3078302e307831",
            "aton=1:7f000001 addr=7f000001 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=7f000001 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 302e302e302e3030",
            "aton=1:00000000 addr=00000000 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=00000000 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3030302e3030302e3030302e303030",
            "aton=1:00000000 addr=00000000 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=00000000 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e342035",
            "aton=1:01020304 addr=01020304 pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 2020",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 612e622e632e64",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3132372e302e302e310078",
            "aton=1:7f000001 addr=7f000001 pton4=1:7f000001 pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=7f000001 ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3a3a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00000000000000000000000000000000 network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3a3a31",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00000000000000000000000000000001 network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 313a3a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00010000000000000000000000000000 network=ffffffff ether=0:01aaaaaaaaaa",
        ),
        (
            "s 313a3a32",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00010000000000000000000000000002 network=ffffffff ether=0:01aaaaaaaaaa",
        ),
        (
            "s 3a3a666666663a312e322e332e34",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00000000000000000000ffff01020304 network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3a3a312e322e332e34",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00000000000000000000000001020304 network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 313a323a333a343a353a363a373a38",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00010002000300040005000600070008 network=ffffffff ether=0:0102030405aa",
        ),
        (
            "s 313a323a333a343a353a363a373a383a39",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:0102030405aa",
        ),
        (
            "s 313a323a333a343a353a363a373a3a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00010002000300040005000600070000 network=ffffffff ether=0:0102030405aa",
        ),
        (
            "s 3a3a323a333a343a353a363a373a38",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00000002000300040005000600070008 network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 313a323a333a343a353a363a373a3a38",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:0102030405aa",
        ),
        (
            "s 313a3a323a3a33",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:01aaaaaaaaaa",
        ),
        (
            "s 3a31",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 313a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:01aaaaaaaaaa",
        ),
        (
            "s 313a3a323a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:01aaaaaaaaaa",
        ),
        (
            "s 3a3a3a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 31323334353a3a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 303030303a303030303a3a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00000000000000000000000000000000 network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 30303030303a3a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 666538303a3a312565746830",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 464538303a3a41",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:fe80000000000000000000000000000a network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3a3a666666663a30312e322e332e34",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3a3a666666663a312e322e33",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 313a323a333a343a353a363a312e322e332e34",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00010002000300040005000601020304 network=ffffffff ether=0:0102030405aa",
        ),
        (
            "s 313a323a333a343a353a363a373a312e322e332e34",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:0102030405aa",
        ),
        (
            "s 3a3a312e322e332e343a35",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 312e322e332e343a3a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 673a3a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 203a3a31",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3a3a3120",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3a3a312e322e332e342e35",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 303a303a303a303a303a303a303a30",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00000000000000000000000000000000 network=ffffffff ether=0:0000000000aa",
        ),
        (
            "s 3a3a666666663a303a30",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00000000000000000000ffff00000000 network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 313a323a333a343a353a3a312e322e332e34",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00010002000300040005000001020304 network=ffffffff ether=0:0102030405aa",
        ),
        (
            "s 313a323a333a343a353a363a373a383a3a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:0102030405aa",
        ),
        (
            "s 3a3a313a323a333a343a353a363a373a38",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 313a323a333a343a353a363a3a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00010002000300040005000600000000 network=ffffffff ether=0:0102030405aa",
        ),
        (
            "s 414243443a656630313a3a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:abcdef01000000000000000000000000 network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3a3a302e302e302e30",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00000000000000000000000000000000 network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 313a323a333a343a353a363a373a2e312e322e33",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:0102030405aa",
        ),
        (
            "s 3a3a666666663a312e322e332e3478",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3a3a312e322e332e3420",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 313a323a333a343a353a363a37",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:0102030405aa",
        ),
        (
            "s 3a3a464646463a3235352e3235352e3235352e323535",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00000000000000000000ffffffffffff network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 3a3a666666663a3235362e312e312e31",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 303a3a30",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00000000000000000000000000000000 network=ffffffff ether=0:00aaaaaaaaaa",
        ),
        (
            "s 313a303a303a303a303a303a303a30",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=1:00010000000000000000000000000000 network=ffffffff ether=0:0100000000aa",
        ),
        (
            "s 5b3a3a315d",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 30303a31313a32323a33333a34343a3535",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=1:001122334455",
        ),
        (
            "s 303a313a323a333a343a35",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=1:000102030405",
        ),
        (
            "s 30303a31313a32323a33333a3434",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:00112233aaaa",
        ),
        (
            "s 30303a31313a32323a33333a34343a35353a3636",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=1:001122334455",
        ),
        (
            "s 41413a62623a43433a64643a45453a6666",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=1:aabbccddeeff",
        ),
        (
            "s 3030303a31313a32323a33333a34343a3535",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 303a313a323a333a343a3520",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=1:000102030405",
        ),
        (
            "s 303a313a323a333a343a3578",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:0001020304aa",
        ),
        (
            "s 30302d31312d32322d33332d34342d3535",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 303031313232333334343535",
            "aton=1:0949b92d addr=0949b92d pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 303a313a323a333a343a3509",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=1:000102030405",
        ),
        (
            "s 303a313a323a333a343a35206a756e6b",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=1:000102030405",
        ),
        (
            "s 663a663a663a663a663a66",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=1:0f0f0f0f0f0f",
        ),
        (
            "s 66663a66663a66663a66663a66663a6667",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:ffffffffffaa",
        ),
        (
            "s 3a313a323a333a343a35",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:aaaaaaaaaaaa",
        ),
        (
            "s 303a313a323a333a343a",
            "aton=0:aaaaaaaa addr=ffffffff pton4=0:aaaaaaaa pton6=0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa network=ffffffff ether=0:0001020304aa",
        ),
        (
            "a6 00000000000000000000000000000000",
            "ntop=:: short=NULL/28",
        ),
        (
            "a6 00000000000000000000000000000001",
            "ntop=::1 short=NULL/28",
        ),
        (
            "a6 00010000000000000000000000000000",
            "ntop=1:: short=NULL/28",
        ),
        (
            "a6 00000000000000000000ffff01020304",
            "ntop=::ffff:1.2.3.4 short=NULL/28",
        ),
        (
            "a6 00000000000000000000000001020304",
            "ntop=::1.2.3.4 short=NULL/28",
        ),
        (
            "a6 00000000000000000000ffff00000000",
            "ntop=::ffff:0.0.0.0 short=NULL/28",
        ),
        (
            "a6 00010000000000010000000000000001",
            "ntop=1:0:0:1::1 short=NULL/28",
        ),
        (
            "a6 00010000000000020000000000030004",
            "ntop=1::2:0:0:3:4 short=NULL/28",
        ),
        (
            "a6 fe800000000000000000000000000001",
            "ntop=fe80::1 short=NULL/28",
        ),
        (
            "a6 20010db8000000000000000000000001",
            "ntop=2001:db8::1 short=NULL/28",
        ),
        (
            "a6 00010002000300040005000600070008",
            "ntop=1:2:3:4:5:6:7:8 short=NULL/28",
        ),
        (
            "a6 00000000000100000000000000000000",
            "ntop=0:0:1:: short=NULL/28",
        ),
        (
            "a6 00000001000000010000000100000001",
            "ntop=0:1:0:1:0:1:0:1 short=NULL/28",
        ),
        (
            "a6 00010000000100000000000100000001",
            "ntop=1:0:1::1:0:1 short=NULL/28",
        ),
        (
            "a6 00000000000000000000000000010000",
            "ntop=::0.1.0.0 short=NULL/28",
        ),
        (
            "a6 0000000000000000ffff000001020304",
            "ntop=::ffff:0:102:304 short=NULL/28",
        ),
        (
            "a6 000000000000000000000000ffff0001",
            "ntop=::255.255.0.1 short=NULL/28",
        ),
        (
            "a6 ffffffffffffffffffffffffffffffff",
            "ntop=ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff short=NULL/28",
        ),
        (
            "a6 00000000000000000001000001020304",
            "ntop=::1:0:102:304 short=NULL/28",
        ),
        (
            "a6 0000000000000000000000000000ffff",
            "ntop=::ffff short=NULL/28",
        ),
        (
            "a6 abcd00000000ef010000000000000000",
            "ntop=abcd:0:0:ef01:: short=NULL/28",
        ),
        (
            "a4 00000000",
            "ntop=0.0.0.0 short=NULL/28 ntoa=0.0.0.0 lnaof=00000000 netof=00000000",
        ),
        (
            "a4 01020304",
            "ntop=1.2.3.4 short=NULL/28 ntoa=1.2.3.4 lnaof=00020304 netof=00000001",
        ),
        (
            "a4 ffffffff",
            "ntop=255.255.255.255 short=NULL/28 ntoa=255.255.255.255 lnaof=000000ff netof=00ffffff",
        ),
        (
            "a4 0a000001",
            "ntop=10.0.0.1 short=NULL/28 ntoa=10.0.0.1 lnaof=00000001 netof=0000000a",
        ),
        (
            "a4 80000001",
            "ntop=128.0.0.1 short=NULL/28 ntoa=128.0.0.1 lnaof=00000001 netof=00008000",
        ),
        (
            "a4 c0a80101",
            "ntop=192.168.1.1 short=NULL/28 ntoa=192.168.1.1 lnaof=00000001 netof=00c0a801",
        ),
        (
            "a4 e0000001",
            "ntop=224.0.0.1 short=NULL/28 ntoa=224.0.0.1 lnaof=00000001 netof=00e00000",
        ),
        (
            "a4 f0000001",
            "ntop=240.0.0.1 short=NULL/28 ntoa=240.0.0.1 lnaof=00000001 netof=00f00000",
        ),
        (
            "a4 7f000001",
            "ntop=127.0.0.1 short=NULL/28 ntoa=127.0.0.1 lnaof=00000001 netof=0000007f",
        ),
        (
            "a4 bfffffff",
            "ntop=191.255.255.255 short=NULL/28 ntoa=191.255.255.255 lnaof=0000ffff netof=0000bfff",
        ),
        (
            "a4 dfffffff",
            "ntop=223.255.255.255 short=NULL/28 ntoa=223.255.255.255 lnaof=000000ff netof=00dfffff",
        ),
        ("mk 7f 1", "makeaddr=7f000001"),
        ("mk 80 1", "makeaddr=00800001"),
        ("mk c0a8 101", "makeaddr=c0a80101"),
        ("mk c0a801 1", "makeaddr=c0a80101"),
        ("mk 1000000 5", "makeaddr=01000005"),
        ("mk 0 0", "makeaddr=00000000"),
        ("mk 7f ffffffff", "makeaddr=7fffffff"),
        ("mk ffff 12345678", "makeaddr=ffff5678"),
        ("mk ffffff ffffff", "makeaddr=ffffffff"),
        ("mk 80 ffffffff", "makeaddr=0080ffff"),
        ("mk ffffffff ffffffff", "makeaddr=ffffffff"),
        ("mk a 20304", "makeaddr=0a020304"),
        ("e 001122334455", "ntoa=0:11:22:33:44:55"),
        ("e 000000000000", "ntoa=0:0:0:0:0:0"),
        ("e ffffffffffff", "ntoa=ff:ff:ff:ff:ff:ff"),
        ("e 0a0b0c0d0e0f", "ntoa=a:b:c:d:e:f"),
        ("e 010203040506", "ntoa=1:2:3:4:5:6"),
        (
            "el 30303a31313a32323a33333a34343a353520686f7374",
            "line=0:001122334455:686f7374",
        ),
        (
            "el 303a313a323a333a343a3509686f737420232063",
            "line=0:000102030405:686f7374",
        ),
        ("el 303a313a323a333a343a35", "line=-1:000102030405:2d"),
        ("el 303a313a323a333a343a3520", "line=-1:000102030405:2d"),
        ("el 303a313a323a333a343a35202378", "line=-1:000102030405:2d"),
        (
            "el 303a313a323a333a343a35686f7374",
            "line=-1:0001020304aa:2d",
        ),
        (
            "el 303a313a323a333a343a353a362068",
            "line=-1:0001020304aa:2d",
        ),
        (
            "el 41413a42423a43433a44443a45453a464620206e616d652e6578616d706c65",
            "line=0:aabbccddeeff:6e616d652e6578616d706c65",
        ),
        ("el 303a313a323a333a342068", "line=-1:00010203aaaa:2d"),
        ("el 303a313a323a333a343a350b68", "line=0:000102030405:68"),
        (
            "el 30303a31313a32323a33333a34343a353509686f73742363",
            "line=0:001122334455:686f7374",
        ),
        ("el 78", "line=-1:aaaaaaaaaaaa:2d"),
        ("el ", "line=-1:aaaaaaaaaaaa:2d"),
    ];

    #[test]
    fn null_strings_are_refused_not_read() {
        let mut a = InAddr { s_addr: 7 };
        // SAFETY: NULL is the input under test; the output is writable.
        unsafe {
            assert_eq!(inet_aton(core::ptr::null(), &mut a), 0);
            assert_eq!(a.s_addr, 7, "untouched");
            assert_eq!(inet_addr(core::ptr::null()), INADDR_NONE);
            assert_eq!(inet_network(core::ptr::null()), INADDR_NONE);
            let mut d = [0u8; 16];
            assert_eq!(inet_pton(AF_INET, core::ptr::null(), d.as_mut_ptr()), 0);
            assert_eq!(
                inet_pton(AF_INET6, c"::1".as_ptr().cast(), core::ptr::null_mut()),
                0
            );
            let mut e = EtherAddr::ZERO;
            assert!(ether_aton_r(core::ptr::null(), &mut e).is_null());
        }
    }

    #[test]
    fn inet_aton_with_no_output_only_checks() {
        // SAFETY: NUL-terminated input; NULL output, which glibc allows.
        unsafe {
            assert_eq!(inet_aton(c"10.1".as_ptr().cast(), core::ptr::null_mut()), 1);
            assert_eq!(inet_aton(c"10.x".as_ptr().cast(), core::ptr::null_mut()), 0);
        }
    }

    #[test]
    fn aton_exact_must_reach_the_end() {
        assert_eq!(aton_exact(b"127.1"), Some([127, 0, 0, 1]));
        assert_eq!(aton_exact(b"0x7f.1"), Some([127, 0, 0, 1]));
        assert_eq!(
            aton_exact(b"127.1 "),
            None,
            "trailing space: inet_aton only"
        );
        assert_eq!(aton_exact(b"127.1 x"), None);
        assert_eq!(aton_exact(b""), None);
    }

    #[test]
    fn inet_pton_of_another_family_is_eafnosupport_whatever_the_pointers() {
        errno::set_errno(0);
        // SAFETY: the pointers are never read for an unknown family.
        let r = unsafe { inet_pton(99, core::ptr::null(), core::ptr::null_mut()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EAFNOSUPPORT);
    }

    #[test]
    fn inet_ntop_bad_family_outranks_null_pointers() {
        errno::set_errno(0);
        assert!(inet_ntop(99, core::ptr::null(), core::ptr::null_mut(), 0).is_null());
        assert_eq!(errno::get_errno(), errno::EAFNOSUPPORT);
    }

    #[test]
    fn inet_ntop_too_small_size_outranks_a_null_destination() {
        let src = [1u8, 2, 3, 4];
        errno::set_errno(0);
        assert!(inet_ntop(AF_INET, src.as_ptr(), core::ptr::null_mut(), 4).is_null());
        assert_eq!(errno::get_errno(), errno::ENOSPC);
    }

    #[test]
    fn inet_ntop_null_source_or_destination_is_efault() {
        errno::set_errno(0);
        let mut out = [0u8; 64];
        assert!(inet_ntop(AF_INET6, core::ptr::null(), out.as_mut_ptr(), 64).is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
        let src = [0u8; 16];
        errno::set_errno(0);
        assert!(inet_ntop(AF_INET6, src.as_ptr(), core::ptr::null_mut(), 64).is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn the_longest_texts_fit_the_standard_sizes() {
        let v4 = ntop4(&[255; 4]);
        assert_eq!(v4.len + 1, INET_ADDRSTRLEN);
        let v6 = ntop6(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 255, 255, 255, 255]);
        assert_eq!(v6.bytes(), b"::ffff:255.255.255.255");
        let full = ntop6(&[0xff; 16]);
        assert!(full.len < INET6_ADDRSTRLEN);
    }

    #[test]
    fn per_thread_buffers_are_the_threads_own() {
        let a = inet_ntoa(InAddr {
            s_addr: u32::from_ne_bytes([10, 0, 0, 1]),
        });
        let other = std::thread::spawn(|| {
            let p = inet_ntoa(InAddr {
                s_addr: u32::from_ne_bytes([192, 168, 0, 1]),
            });
            (p as usize, text(p))
        })
        .join()
        .unwrap();
        assert_ne!(a as usize, other.0);
        assert_eq!(other.1, "192.168.0.1");
        assert_eq!(text(a), "10.0.0.1", "not overwritten by the other thread");

        // SAFETY: NUL-terminated input.
        let e = unsafe { ether_aton(c"1:2:3:4:5:6".as_ptr().cast()) };
        assert!(!e.is_null());
        // SAFETY: `e` is this thread's buffer, just filled.
        assert_eq!(unsafe { (*e).ether_addr_octet }, [1, 2, 3, 4, 5, 6]);
        // SAFETY: `e` is readable.
        assert_eq!(text(unsafe { ether_ntoa(e) }), "1:2:3:4:5:6");
    }
}
