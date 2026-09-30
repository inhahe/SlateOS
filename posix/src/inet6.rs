//! `<netinet/in.h>`'s helpers for IPv6 extension headers and multicast:
//!
//! - RFC 3542's `inet6_opt_*`, which build and read the options of a
//!   Hop-by-Hop or Destination Options header, and `inet6_rth_*`, which build
//!   and read a Type 0 Routing header;
//! - RFC 2292's older `inet6_option_*`, the same over a `cmsghdr` of
//!   ancillary data;
//! - `bindresvport`, which binds a socket to a free privileged port;
//! - the multicast source filters -- `getsourcefilter`, `setsourcefilter`
//!   and their IPv4 pair -- which are refused, `ENOPROTOOPT`: the sockets
//!   here keep no source filters, and `setsockopt` accepts an option it does
//!   not know without acting on it, so a filter set through it would be one
//!   the program believes in and nothing applies (design-decisions §1144's
//!   rule: refuse rather than pretend).
//!
//! Written from the RFCs. glibc 2.39's answers are replayed
//! (`posix/tools/oracle/inet6_harness.py`, `inet6_oracle.txt`): RFC 3542's
//! functions are glibc's exactly. RFC 2292's are the RFC's where glibc's
//! part from it, each difference on purpose:
//!
//! - glibc's pads each option to its `xn + y` alignment by rounding up to a
//!   multiple of `x` and then adding `y`, so an option that needs no padding
//!   gets `y` bytes of it, and it keeps each option's tail padding in the
//!   header; here the padding is the least that aligns the option, and the
//!   tail padding moves to the new end -- the layouts of RFC 2292 section
//!   6.3.7's own examples, which the tests build byte for byte.
//! - glibc's `inet6_option_alloc` reserves `datalen` bytes; RFC 2292 makes
//!   `datalen` "the value of the option data length byte", so here the
//!   reservation is that and the type and length bytes, 2 more.
//! - glibc's `inet6_option_next` leaves `*tptrp` past the last option when
//!   it has none left; the RFC says NULL there, and a pointer not NULL means
//!   an error.

// Offsets and lengths here are inside a header the caller has checked or
// built with these functions, of at most 2,048 bytes; each is compared with
// the header's length before it is used.
#![allow(clippy::arithmetic_side_effects)]

use core::ptr::null_mut;

use crate::errno::{EADDRINUSE, EAFNOSUPPORT, EINVAL, ENOPROTOOPT, get_errno, set_errno};

/// The Pad1 option: one byte of padding, no length.
const PAD1: u8 = 0;
/// The PadN option: its length byte, and that many zero bytes.
const PADN: u8 = 1;

/// Write `n` bytes of padding at `p`: Pad1 for one, PadN for more.
///
/// # Safety
///
/// `p` has `n` writable bytes; `n` is at most 257.
unsafe fn pad(p: *mut u8, n: usize) {
    // SAFETY: the caller's `n` bytes.
    unsafe {
        match n {
            0 => {}
            1 => p.write(PAD1),
            _ => {
                p.write(PADN);
                p.add(1).write((n - 2) as u8);
                core::ptr::write_bytes(p.add(2), 0, n - 2);
            }
        }
    }
}

/// `n` rounded up to a multiple of `to` (a power of two).
const fn round_up(n: usize, to: usize) -> usize {
    (n + to - 1) & !(to - 1)
}

// ---------------------------------------------------------------------------
// RFC 3542: inet6_opt_*
// ---------------------------------------------------------------------------

/// The largest extension header: its length byte counts 8-byte units past
/// the first, 255 of them.
const EXT_MAX: u32 = 2048;

/// `inet6_opt_init` (RFC 3542 10.1): the space an empty options header
/// takes, 2; and with a buffer, its length byte set for `extlen` bytes, which
/// must be a positive multiple of 8 (-1 otherwise).
///
/// # Safety
///
/// `extbuf` is NULL or `extlen` writable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_opt_init(extbuf: *mut u8, extlen: u32) -> i32 {
    if !extbuf.is_null() {
        if extlen == 0 || extlen % 8 != 0 || extlen > EXT_MAX {
            return -1;
        }
        // SAFETY: at least 8 bytes.
        unsafe { extbuf.add(1).write((extlen / 8 - 1) as u8) };
    }
    2
}

/// `inet6_opt_append` (RFC 3542 10.2): where an option of `type`, `len` data
/// bytes and alignment `align` (1, 2, 4 or 8, and not more than `len`) ends
/// when added at `offset` -- after the least padding that puts its data on a
/// multiple of `align`. With a buffer, the padding and the option's two bytes
/// are written, its data's place given back in `*databufp`, and -1 for one
/// that does not fit. -1 too for a `type` of 0 or 1 (Pad1, PadN), a `len`
/// past 255, or an `offset` before the first option.
///
/// # Safety
///
/// `extbuf` is NULL or `extlen` writable bytes; `databufp` NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_opt_append(
    extbuf: *mut u8,
    extlen: u32,
    offset: i32,
    typ: u8,
    len: u32,
    align: u8,
    databufp: *mut *mut u8,
) -> i32 {
    let Ok(offset) = usize::try_from(offset) else {
        return -1;
    };
    if offset < 2
        || typ < 2
        || len > 255
        || !matches!(align, 1 | 2 | 4 | 8)
        || u32::from(align) > len
    {
        return -1;
    }
    let data = round_up(offset + 2, align as usize);
    let end = data + len as usize;
    if !extbuf.is_null() {
        if end > extlen as usize {
            return -1;
        }
        // SAFETY: all of it inside the caller's `extlen` bytes.
        unsafe {
            pad(extbuf.add(offset), data - 2 - offset);
            extbuf.add(data - 2).write(typ);
            extbuf.add(data - 1).write(len as u8);
            if !databufp.is_null() {
                *databufp = extbuf.add(data);
            }
        }
    }
    end as i32
}

/// `inet6_opt_finish` (RFC 3542 10.3): the header's length, `offset` padded
/// to a multiple of 8; with a buffer, the padding written, and -1 if it does
/// not fit.
///
/// # Safety
///
/// `extbuf` is NULL or `extlen` writable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_opt_finish(extbuf: *mut u8, extlen: u32, offset: i32) -> i32 {
    let Ok(offset) = usize::try_from(offset) else {
        return -1;
    };
    if offset < 2 {
        return -1;
    }
    let end = round_up(offset, 8);
    if !extbuf.is_null() {
        if end > extlen as usize {
            return -1;
        }
        // SAFETY: inside the caller's `extlen` bytes.
        unsafe { pad(extbuf.add(offset), end - offset) };
    }
    end as i32
}

/// `inet6_opt_set_val` (RFC 3542 10.4): `vallen` bytes of `val` into the
/// option data at `databuf`, `offset` in; the offset after them.
///
/// # Safety
///
/// `databuf + offset` has `vallen` writable bytes; `val` `vallen` readable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_opt_set_val(
    databuf: *mut u8,
    offset: i32,
    val: *const u8,
    vallen: u32,
) -> i32 {
    let Ok(at) = usize::try_from(offset) else {
        return -1;
    };
    // SAFETY: the caller's contract; the two may be unaligned, and are
    // copied as bytes, as the RFC requires.
    unsafe { core::ptr::copy(val, databuf.add(at), vallen as usize) };
    offset + vallen as i32
}

/// `inet6_opt_get_val` (RFC 3542 10.7): `vallen` bytes of the option data at
/// `databuf`, `offset` in, into `val`; the offset after them.
///
/// # Safety
///
/// `databuf + offset` has `vallen` readable bytes; `val` `vallen` writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_opt_get_val(
    databuf: *const u8,
    offset: i32,
    val: *mut u8,
    vallen: u32,
) -> i32 {
    let Ok(at) = usize::try_from(offset) else {
        return -1;
    };
    // SAFETY: the caller's contract, as above.
    unsafe { core::ptr::copy(databuf.add(at), val, vallen as usize) };
    offset + vallen as i32
}

/// The option after `offset` in the header's `extlen` bytes that is not
/// padding: its offset, type, data length and end; `None` at the end or for
/// a malformed header.
///
/// # Safety
///
/// `extbuf` has `extlen` readable bytes.
unsafe fn next_option(
    extbuf: *const u8,
    extlen: usize,
    offset: usize,
) -> Option<(usize, u8, usize, usize)> {
    // SAFETY: the caller's `extlen` bytes, each read checked against it.
    let at = |i: usize| unsafe { extbuf.add(i).read() };
    let mut off = if offset == 0 { 2 } else { offset };
    while off < extlen {
        let typ = at(off);
        if typ == PAD1 {
            off += 1;
            continue;
        }
        if off + 2 > extlen {
            return None;
        }
        let len = usize::from(at(off + 1));
        let end = off + 2 + len;
        if end > extlen {
            return None;
        }
        if typ != PADN {
            return Some((off, typ, len, end));
        }
        off = end;
    }
    None
}

/// `inet6_opt_next` (RFC 3542 10.5): the option after `offset` -- 0 for the
/// first -- that is not padding: its type, data length and data into
/// `*typep`, `*lenp` and `*databufp`, and the offset past it; -1 at the end
/// or for a malformed header.
///
/// # Safety
///
/// `extbuf` has `extlen` readable bytes; the three out-pointers writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_opt_next(
    extbuf: *mut u8,
    extlen: u32,
    offset: i32,
    typep: *mut u8,
    lenp: *mut u32,
    databufp: *mut *mut u8,
) -> i32 {
    let Ok(offset) = usize::try_from(offset) else {
        return -1;
    };
    if extbuf.is_null() {
        return -1;
    }
    // SAFETY: the caller's header.
    match unsafe { next_option(extbuf, extlen as usize, offset) } {
        Some((off, typ, len, end)) => {
            // SAFETY: the caller's places; the data inside the header.
            unsafe {
                typep.write(typ);
                lenp.write(len as u32);
                databufp.write(extbuf.add(off + 2));
            }
            end as i32
        }
        None => -1,
    }
}

/// `inet6_opt_find` (RFC 3542 10.6): as [`inet6_opt_next`], but the next
/// option of `typ`.
///
/// # Safety
///
/// As [`inet6_opt_next`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_opt_find(
    extbuf: *mut u8,
    extlen: u32,
    offset: i32,
    typ: u8,
    lenp: *mut u32,
    databufp: *mut *mut u8,
) -> i32 {
    let Ok(mut offset) = usize::try_from(offset) else {
        return -1;
    };
    if extbuf.is_null() {
        return -1;
    }
    // SAFETY: the caller's header.
    while let Some((off, t, len, end)) = unsafe { next_option(extbuf, extlen as usize, offset) } {
        if t == typ {
            // SAFETY: the caller's places; the data inside the header.
            unsafe {
                lenp.write(len as u32);
                databufp.write(extbuf.add(off + 2));
            }
            return end as i32;
        }
        offset = end;
    }
    -1
}

// ---------------------------------------------------------------------------
// RFC 3542: inet6_rth_* (the Type 0 Routing header)
// ---------------------------------------------------------------------------

/// `IPV6_RTHDR_TYPE_0`.
const RTHDR_TYPE_0: i32 = 0;
/// A Type 0 header's fixed part: next header, length, type, segments left,
/// and 4 reserved bytes.
const RTH0_HEAD: usize = 8;
/// An address.
const ADDR: usize = 16;

/// `inet6_rth_space` (RFC 3542 7.1): the bytes a Type 0 Routing header of
/// `segments` addresses takes, 0 to 127 of them; 0 for another type or count.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inet6_rth_space(typ: i32, segments: i32) -> u32 {
    if typ != RTHDR_TYPE_0 || !(0..=127).contains(&segments) {
        return 0;
    }
    (RTH0_HEAD + ADDR * segments as usize) as u32
}

/// `inet6_rth_init` (RFC 3542 7.2): a Type 0 header of room for `segments`
/// addresses in `bp`, zeroed, segments left 0; `bp`, or NULL for a type or
/// count [`inet6_rth_space`] refuses or a `bp_len` too small.
///
/// # Safety
///
/// `bp` has `bp_len` writable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_rth_init(
    bp: *mut u8,
    bp_len: u32,
    typ: i32,
    segments: i32,
) -> *mut u8 {
    let space = inet6_rth_space(typ, segments);
    if space == 0 || bp.is_null() || bp_len < space {
        return null_mut();
    }
    // SAFETY: `space` of the caller's `bp_len` bytes.
    unsafe {
        core::ptr::write_bytes(bp, 0, space as usize);
        bp.add(1).write((segments * 2) as u8);
        bp.add(2).write(typ as u8);
    }
    bp
}

/// A Type 0 header's number of addresses, from its length byte; `None` for
/// another type or an odd length.
///
/// # Safety
///
/// `bp` has the header's first 4 bytes readable.
unsafe fn rth0_segments(bp: *const u8) -> Option<usize> {
    // SAFETY: the caller's contract.
    let (len, typ) = unsafe { (bp.add(1).read(), bp.add(2).read()) };
    (i32::from(typ) == RTHDR_TYPE_0 && len % 2 == 0).then_some(usize::from(len / 2))
}

/// `inet6_rth_add` (RFC 3542 7.3): `addr` after the addresses added so far,
/// segments left one more; 0, or -1 when the header is full.
///
/// # Safety
///
/// `bp` a header [`inet6_rth_init`] made, `addr` 16 readable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_rth_add(bp: *mut u8, addr: *const u8) -> i32 {
    // SAFETY: the caller's header.
    let Some(room) = (unsafe { rth0_segments(bp) }) else {
        return -1;
    };
    // SAFETY: as above.
    let left = usize::from(unsafe { bp.add(3).read() });
    if left >= room {
        return -1;
    }
    // SAFETY: address `left` of the header's `room`.
    unsafe {
        core::ptr::copy(addr, bp.add(RTH0_HEAD + ADDR * left), ADDR);
        bp.add(3).write((left + 1) as u8);
    }
    0
}

/// `inet6_rth_reverse` (RFC 3542 7.4): into `out` the header `in_` with its
/// addresses in the other order and segments left all of them; `in_` and
/// `out` may be one buffer. 0, or -1 for a header not Type 0.
///
/// # Safety
///
/// `in_` a Type 0 header; `out` room for one as long.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_rth_reverse(in_: *const u8, out: *mut u8) -> i32 {
    // SAFETY: the caller's header.
    let Some(n) = (unsafe { rth0_segments(in_) }) else {
        return -1;
    };
    // The addresses first, so that `out` may be `in_`.
    let mut addrs = [[0u8; ADDR]; 128];
    for (i, slot) in addrs.iter_mut().enumerate().take(n) {
        // SAFETY: address `i` of the header's `n`.
        unsafe { core::ptr::copy(in_.add(RTH0_HEAD + ADDR * i), slot.as_mut_ptr(), ADDR) };
    }
    // SAFETY: the caller's two headers; the fixed part copied, then the
    // addresses written back reversed.
    unsafe {
        core::ptr::copy(in_, out, RTH0_HEAD);
        out.add(3).write(n as u8);
        for (i, a) in addrs.iter().take(n).rev().enumerate() {
            core::ptr::copy(a.as_ptr(), out.add(RTH0_HEAD + ADDR * i), ADDR);
        }
    }
    0
}

/// `inet6_rth_segments` (RFC 3542 7.5): how many addresses the header holds;
/// -1 for one not Type 0.
///
/// # Safety
///
/// `bp` a Routing header's first 4 bytes, readable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_rth_segments(bp: *const u8) -> i32 {
    // SAFETY: the caller's contract.
    unsafe { rth0_segments(bp) }.map_or(-1, |n| n as i32)
}

/// `inet6_rth_getaddr` (RFC 3542 7.6): address `index`, from 0 to one less
/// than [`inet6_rth_segments`]; NULL outside that.
///
/// # Safety
///
/// `bp` a Type 0 header, all of it readable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_rth_getaddr(bp: *const u8, index: i32) -> *mut u8 {
    // SAFETY: the caller's header.
    let n = unsafe { rth0_segments(bp) }.unwrap_or(0);
    match usize::try_from(index) {
        // SAFETY: address `i` of the header's `n`.
        Ok(i) if i < n => unsafe { bp.add(RTH0_HEAD + ADDR * i).cast_mut() },
        _ => null_mut(),
    }
}

// ---------------------------------------------------------------------------
// RFC 2292: inet6_option_* (ancillary data)
// ---------------------------------------------------------------------------

/// `struct cmsghdr`: its length (with the header), level and type.
#[repr(C)]
struct Cmsghdr {
    len: usize,
    level: i32,
    typ: i32,
}

/// `CMSG_LEN(0)`: the header, which the data follows.
const CMSG_HEAD: usize = core::mem::size_of::<Cmsghdr>();
/// `IPPROTO_IPV6`.
const IPPROTO_IPV6: i32 = 41;
/// `IPV6_HOPOPTS`.
const IPV6_HOPOPTS: i32 = 54;
/// `IPV6_DSTOPTS`.
const IPV6_DSTOPTS: i32 = 59;

/// `inet6_option_space` (RFC 2292 6.3.1): the ancillary data an options
/// header of one option of `nbytes` bytes -- its leading padding, type,
/// length and data -- takes, its header and the extension header's two bytes
/// included, padded to a multiple of 8: `CMSG_SPACE`, as glibc's counts it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inet6_option_space(nbytes: i32) -> i32 {
    let n = usize::try_from(nbytes).unwrap_or(0);
    (CMSG_HEAD + round_up(round_up(n + 2, 8), 8)) as i32
}

/// `inet6_option_init` (RFC 2292 6.3.2): a `cmsghdr` at `bp` for a
/// Hop-by-Hop (`IPV6_HOPOPTS`) or Destination (`IPV6_DSTOPTS`) options
/// header with no options yet, into `*cmsgp`; 0, or -1 for another type.
///
/// # Safety
///
/// `bp` has room for the ancillary data (see [`inet6_option_space`]) and is
/// aligned for a `cmsghdr`; `cmsgp` writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_option_init(bp: *mut u8, cmsgp: *mut *mut u8, typ: i32) -> i32 {
    if typ != IPV6_HOPOPTS && typ != IPV6_DSTOPTS {
        return -1;
    }
    // SAFETY: the caller's buffer, aligned for the header.
    unsafe {
        bp.cast::<Cmsghdr>().write(Cmsghdr {
            len: CMSG_HEAD,
            level: IPPROTO_IPV6,
            typ,
        });
        cmsgp.write(bp);
    }
    0
}

/// Where the options of the extension header in `ext`, `used` bytes of it,
/// really end: past the last option that is not padding -- so that the
/// padding a previous option ended with moves to the new end rather than
/// staying in the middle.
///
/// # Safety
///
/// `ext` has `used` readable bytes.
unsafe fn options_end(ext: *const u8, used: usize) -> usize {
    let mut end = 2;
    let mut off = 2;
    // SAFETY: each read below `used`.
    let at = |i: usize| unsafe { ext.add(i).read() };
    while off < used {
        let typ = at(off);
        let next = if typ == PAD1 {
            off + 1
        } else if off + 2 <= used {
            off + 2 + usize::from(at(off + 1))
        } else {
            used
        };
        if typ != PAD1 && typ != PADN && next <= used {
            end = next;
        }
        off = next;
    }
    end
}

/// `inet6_option_alloc`'s work, and `inet6_option_append`'s: room for an
/// option of `datalen` data bytes at the least padding that puts its type
/// byte on `xn + y`, the header padded after it to a multiple of 8.
///
/// # Safety
///
/// `cmsg` a header [`inet6_option_init`] made, with room past it.
unsafe fn option_alloc(cmsg: *mut u8, datalen: usize, multx: i32, plusy: i32) -> *mut u8 {
    if !matches!(multx, 1 | 2 | 4 | 8) || !(0..=7).contains(&plusy) || datalen > 255 {
        return null_mut();
    }
    let (x, y) = (multx as usize, plusy as usize % multx as usize);
    let hdr = cmsg.cast::<Cmsghdr>();
    // SAFETY: the caller's header, and the data after it.
    unsafe {
        let ext = cmsg.add(CMSG_HEAD);
        let used = (*hdr).len.saturating_sub(CMSG_HEAD);
        let from = if used < 2 {
            // The first option: the Next Header and Hdr Ext Len bytes first.
            ext.write(0);
            ext.add(1).write(0);
            2
        } else {
            options_end(ext, used)
        };
        let mut start = from;
        while start % x != y {
            start += 1;
        }
        pad(ext.add(from), start - from);
        let end = start + 2 + datalen;
        core::ptr::write_bytes(ext.add(start), 0, 2 + datalen);
        let total = round_up(end, 8);
        pad(ext.add(end), total - end);
        ext.add(1).write((total / 8 - 1) as u8);
        (*hdr).len = CMSG_HEAD + total;
        ext.add(start)
    }
}

/// `inet6_option_append` (RFC 2292 6.3.3): the option at `typep` -- its
/// type, length and data -- added to the header, at the least padding that
/// puts it on `multx * n + plusy`, and the header padded after it to a
/// multiple of 8. 0; -1 for a Pad1 or PadN type, a `multx` not 1, 2, 4 or 8,
/// or a `plusy` past 7.
///
/// # Safety
///
/// `cmsg` a header [`inet6_option_init`] made, with room for the option;
/// `typep` its type, length and data, readable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_option_append(
    cmsg: *mut u8,
    typep: *const u8,
    multx: i32,
    plusy: i32,
) -> i32 {
    // SAFETY: the caller's option.
    let (typ, len) = unsafe { (typep.read(), usize::from(typep.add(1).read())) };
    if typ < 2 {
        return -1;
    }
    // SAFETY: the caller's header.
    let p = unsafe { option_alloc(cmsg, len, multx, plusy) };
    if p.is_null() {
        return -1;
    }
    // SAFETY: the option's 2 + len bytes, into the room just made.
    unsafe { core::ptr::copy(typep, p, 2 + len) };
    0
}

/// `inet6_option_alloc` (RFC 2292 6.3.4): room for an option whose data
/// length byte will be `datalen`, as [`inet6_option_append`] places one, for
/// the caller to write its type, length and data into -- zeroed, so that
/// until then it reads as padding; a pointer to it, or NULL for a bad
/// `multx`, `plusy` or `datalen`.
///
/// # Safety
///
/// As [`inet6_option_append`]'s `cmsg`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_option_alloc(
    cmsg: *mut u8,
    datalen: i32,
    multx: i32,
    plusy: i32,
) -> *mut u8 {
    let Ok(datalen) = usize::try_from(datalen) else {
        return null_mut();
    };
    // SAFETY: the caller's header.
    unsafe { option_alloc(cmsg, datalen, multx, plusy) }
}

/// The options header in `cmsg`: its bytes' start and length, if `cmsg` is
/// one -- level `IPPROTO_IPV6`, type `IPV6_HOPOPTS` or `IPV6_DSTOPTS`, its
/// length byte inside the data.
///
/// # Safety
///
/// `cmsg` a readable `cmsghdr` and its data.
unsafe fn options_of(cmsg: *const u8) -> Option<(*const u8, usize)> {
    let hdr = cmsg.cast::<Cmsghdr>();
    // SAFETY: the caller's header and data.
    unsafe {
        if (*hdr).level != IPPROTO_IPV6
            || ((*hdr).typ != IPV6_HOPOPTS && (*hdr).typ != IPV6_DSTOPTS)
        {
            return None;
        }
        let used = (*hdr).len.checked_sub(CMSG_HEAD)?;
        let ext = cmsg.add(CMSG_HEAD);
        if used < 2 {
            return None;
        }
        let hdr_len = (usize::from(ext.add(1).read()) + 1) * 8;
        (hdr_len <= used).then_some((ext, hdr_len))
    }
}

/// `inet6_option_next` (RFC 2292 6.3.5): the option after `*tptrp` -- the
/// first for a NULL one -- padding included, as glibc's returns it: 0 and
/// `*tptrp` at its type byte; -1 and `*tptrp` NULL when none is left, as the
/// RFC says; -1 and `*tptrp` not NULL for a malformed header.
///
/// # Safety
///
/// `cmsg` a readable `cmsghdr` and its data; `tptrp` readable and writable,
/// `*tptrp` NULL or an option of that header.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_option_next(cmsg: *const u8, tptrp: *mut *mut u8) -> i32 {
    // SAFETY: the caller's header.
    let Some((ext, len)) = (unsafe { options_of(cmsg) }) else {
        return -1;
    };
    // SAFETY: the caller's place, and bytes of the header.
    unsafe {
        let at = |i: usize| ext.add(i).read();
        let cur = *tptrp;
        let off = if cur.is_null() {
            2
        } else {
            let o = (cur as usize).wrapping_sub(ext as usize);
            if o >= len {
                return -1;
            }
            if at(o) == PAD1 {
                o + 1
            } else {
                o + 2 + usize::from(at(o + 1))
            }
        };
        if off >= len {
            *tptrp = null_mut();
            return -1;
        }
        if at(off) != PAD1 && (off + 2 > len || off + 2 + usize::from(at(off + 1)) > len) {
            *tptrp = ext.add(off).cast_mut();
            return -1;
        }
        *tptrp = ext.add(off).cast_mut();
    }
    0
}

/// `inet6_option_find` (RFC 2292 6.3.6): as [`inet6_option_next`], but the
/// next option of `typ`; -1 and `*tptrp` NULL when there is none.
///
/// # Safety
///
/// As [`inet6_option_next`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn inet6_option_find(cmsg: *const u8, tptrp: *mut *mut u8, typ: i32) -> i32 {
    loop {
        // SAFETY: the caller's header and place.
        if unsafe { inet6_option_next(cmsg, tptrp) } != 0 {
            return -1;
        }
        // SAFETY: an option's type byte, just found.
        if i32::from(unsafe { (*tptrp).read() }) == typ {
            return 0;
        }
    }
}

// ---------------------------------------------------------------------------
// bindresvport
// ---------------------------------------------------------------------------

/// Where the next `bindresvport` begins its search, so that successive calls
/// do not all try the same port first.
static NEXT_PORT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);

/// `bindresvport` (BSD): bind `sd` to a free privileged port -- one from 600
/// to 1023, then from 512 to 599 -- at `sin`'s address, or the wildcard for a
/// NULL `sin`; 0, or -1 with `bind`'s error -- `EACCES` for a process not
/// allowed a privileged port -- or `EADDRINUSE` when every port is taken.
/// A `sin` not `AF_INET` is `EAFNOSUPPORT`. On success `sin`'s port is the
/// one bound.
///
/// # Safety
///
/// `sin` is NULL or a writable `struct sockaddr_in`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn bindresvport(sd: i32, sin: *mut crate::socket::SockaddrIn) -> i32 {
    use crate::socket::{AF_INET, InAddr, SockaddrIn, bind};
    let mut wildcard = SockaddrIn {
        sin_family: AF_INET as u16,
        sin_port: 0,
        sin_addr: InAddr { s_addr: 0 },
        sin_zero: [0; 8],
    };
    let addr = if sin.is_null() {
        &raw mut wildcard
    } else {
        sin
    };
    // SAFETY: the caller's address, or ours.
    if unsafe { (*addr).sin_family } != AF_INET as u16 {
        set_errno(EAFNOSUPPORT);
        return -1;
    }
    // 600..=1023 then 512..=599: 512 ports, walked from a start that moves.
    let first = NEXT_PORT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) % 512;
    for i in 0..512u16 {
        let k = (first + i) % 512;
        let port = if k < 424 { 600 + k } else { 512 + (k - 424) };
        // SAFETY: as above.
        unsafe { (*addr).sin_port = port.to_be() };
        // SAFETY: a whole `struct sockaddr_in`.
        let r = unsafe { bind(sd, addr.cast(), core::mem::size_of::<SockaddrIn>() as u32) };
        if r == 0 {
            return 0;
        }
        if get_errno() != EADDRINUSE {
            return -1;
        }
    }
    set_errno(EADDRINUSE);
    -1
}

// ---------------------------------------------------------------------------
// The multicast source filters: refused
// ---------------------------------------------------------------------------

/// The checks a source-filter call makes before it is refused: `s` a socket
/// (`EBADF`, `ENOTSOCK`), then `ENOPROTOOPT` -- the sockets here keep no
/// source filters.
fn refuse_filter(s: i32) -> i32 {
    let mut typ: i32 = 0;
    let mut len = core::mem::size_of::<i32>() as u32;
    // SAFETY: an `int` and its length, ours.
    let r = unsafe {
        crate::socket::getsockopt(
            s,
            crate::socket::SOL_SOCKET,
            crate::socket::SO_TYPE,
            (&raw mut typ).cast(),
            &raw mut len,
        )
    };
    if r != 0 {
        return -1;
    }
    set_errno(ENOPROTOOPT);
    -1
}

/// The level of a source filter's group: `IPPROTO_IP` for an IPv4 group of
/// `grouplen` bytes enough, `IPPROTO_IPV6` for an IPv6 one; `None` (glibc's
/// `EINVAL`) otherwise.
///
/// # Safety
///
/// `group` is NULL or `grouplen` readable bytes.
unsafe fn group_ok(group: *const u8, grouplen: u32) -> bool {
    if group.is_null() || grouplen < 2 {
        return false;
    }
    // SAFETY: the family's two bytes.
    let family = i32::from(unsafe { group.cast::<u16>().read_unaligned() });
    match family {
        f if f == crate::socket::AF_INET => grouplen >= 16,
        f if f == crate::socket::AF_INET6 => grouplen >= 28,
        _ => false,
    }
}

/// `getipv4sourcefilter` (RFC 3678): refused -- see [`refuse_filter`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getipv4sourcefilter(
    s: i32,
    _interface: u32,
    _group: u32,
    _fmode: *mut u32,
    _numsrc: *mut u32,
    _slist: *mut u32,
) -> i32 {
    refuse_filter(s)
}

/// `setipv4sourcefilter` (RFC 3678): refused -- see [`refuse_filter`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setipv4sourcefilter(
    s: i32,
    _interface: u32,
    _group: u32,
    _fmode: u32,
    _numsrc: u32,
    _slist: *const u32,
) -> i32 {
    refuse_filter(s)
}

/// `getsourcefilter` (RFC 3678): `EINVAL` for a group that is neither a whole
/// IPv4 nor IPv6 address, as glibc's; else refused -- see [`refuse_filter`].
///
/// # Safety
///
/// `group` is NULL or `grouplen` readable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getsourcefilter(
    s: i32,
    _interface: u32,
    group: *const u8,
    grouplen: u32,
    _fmode: *mut u32,
    _numsrc: *mut u32,
    _slist: *mut u8,
) -> i32 {
    // SAFETY: this function's contract.
    if !unsafe { group_ok(group, grouplen) } {
        set_errno(EINVAL);
        return -1;
    }
    refuse_filter(s)
}

/// `setsourcefilter` (RFC 3678): as [`getsourcefilter`].
///
/// # Safety
///
/// As [`getsourcefilter`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn setsourcefilter(
    s: i32,
    _interface: u32,
    group: *const u8,
    grouplen: u32,
    _fmode: u32,
    _numsrc: u32,
    _slist: *const u8,
) -> i32 {
    // SAFETY: this function's contract.
    if !unsafe { group_ok(group, grouplen) } {
        set_errno(EINVAL);
        return -1;
    }
    refuse_filter(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::null;

    /// glibc 2.39's answers (`posix/tools/oracle/inet6_harness.py`).
    const ORACLE: &str = include_str!("inet6_oracle.txt");

    fn oracle(name: &str) -> &'static str {
        ORACLE
            .lines()
            .find_map(|l| l.strip_prefix(name)?.strip_prefix(" ="))
            .unwrap_or_else(|| panic!("no `{name}` in the oracle"))
    }

    fn hex(b: &[u8]) -> String {
        format!(
            " [{}]",
            b.iter()
                .map(|x| format!("{x:02x}"))
                .collect::<Vec<_>>()
                .concat()
        )
    }

    fn unhex(h: &str) -> Vec<u8> {
        (0..h.len() / 2)
            .map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    fn at(base: *const u8, p: *const u8) -> String {
        if p.is_null() {
            " NULL".into()
        } else {
            format!(" @{}", p as usize - base as usize)
        }
    }

    #[test]
    fn opt_space_and_build_are_glibcs() {
        unsafe {
            let mut got = String::new();
            let mut off = inet6_opt_init(null_mut(), 0);
            got += &format!(" {off}");
            for (t, l, a) in [(0x10, 4, 4), (0x11, 1, 1), (0x12, 8, 8)] {
                off = inet6_opt_append(null_mut(), 0, off, t, l, a, null_mut());
                got += &format!(" {off}");
            }
            got += &format!(" {}", inet6_opt_finish(null_mut(), 0, off));
            assert_eq!(got, oracle("opt-space"));

            let mut buf = [0xEEu8; 64];
            let b = buf.as_mut_ptr();
            let mut data = null_mut();
            let mut got = String::new();
            let mut off = inet6_opt_init(b, 32);
            got += &format!(" {off}");
            off = inet6_opt_append(b, 32, off, 0x10, 4, 4, &raw mut data);
            got += &(format!(" {off}") + &at(b, data));
            got += &format!(
                " {}",
                inet6_opt_set_val(data, 0, 0x1122_3344u32.to_ne_bytes().as_ptr(), 4)
            );
            off = inet6_opt_append(b, 32, off, 0x11, 1, 1, &raw mut data);
            got += &(format!(" {off}") + &at(b, data));
            got += &format!(" {}", inet6_opt_set_val(data, 0, [0x55u8].as_ptr(), 1));
            off = inet6_opt_append(b, 32, off, 0x12, 8, 8, &raw mut data);
            got += &(format!(" {off}") + &at(b, data));
            let v64 = 0x0102_0304_0506_0708u64;
            got += &format!(
                " {}",
                inet6_opt_set_val(data, 0, v64.to_ne_bytes().as_ptr(), 8)
            );
            off = inet6_opt_finish(b, 32, off);
            got += &(format!(" {off}") + &hex(&buf[..40]));
            assert_eq!(got, oracle("opt-build"));

            let mut got = String::new();
            let (mut typ, mut len) = (0u8, 0u32);
            let mut o = 0;
            loop {
                o = inet6_opt_next(b, 32, o, &raw mut typ, &raw mut len, &raw mut data);
                if o == -1 {
                    break;
                }
                got += &(format!(" {o}:{typ:02x}:{len}") + &at(b, data));
            }
            assert_eq!(got + " end", oracle("opt-next"));

            let o = inet6_opt_find(b, 32, 0, 0x12, &raw mut len, &raw mut data);
            let mut got = format!(" {o}:{len}") + &at(b, data);
            let mut back = [0u8; 8];
            let r = inet6_opt_get_val(data, 0, back.as_mut_ptr(), 8);
            got += &format!(" {r} {}", i32::from(u64::from_ne_bytes(back) == v64));
            got += &format!(
                " {}",
                inet6_opt_find(b, 32, 0, 0x99, &raw mut len, &raw mut data)
            );
            assert_eq!(got, oracle("opt-find"));
        }
    }

    #[test]
    fn opt_refusals_and_padding_are_glibcs() {
        unsafe {
            let mut buf = [0u8; 64];
            let b = buf.as_mut_ptr();
            let mut data = null_mut();
            let got = [
                inet6_opt_init(b, 12),
                inet6_opt_append(null_mut(), 0, 2, 0, 4, 4, null_mut()),
                inet6_opt_append(null_mut(), 0, 2, 1, 4, 4, null_mut()),
                inet6_opt_append(null_mut(), 0, 2, 5, 4, 3, null_mut()),
                inet6_opt_append(null_mut(), 0, 2, 5, 2, 4, null_mut()),
                inet6_opt_append(null_mut(), 0, 2, 5, 256, 1, null_mut()),
                inet6_opt_append(null_mut(), 0, 1, 5, 4, 4, null_mut()),
                inet6_opt_init(b, 8),
                inet6_opt_append(b, 8, 2, 5, 8, 8, &raw mut data),
                inet6_opt_finish(b, 4, 6),
                inet6_opt_append(null_mut(), 0, 2, 5, 0, 1, null_mut()),
            ]
            .iter()
            .map(|r| format!(" {r}"))
            .collect::<Vec<_>>()
            .concat();
            assert_eq!(got, oracle("opt-refused"));
            for pre in 0..=7u32 {
                let mut buf = [0xEEu8; 64];
                let b = buf.as_mut_ptr();
                let mut off = inet6_opt_init(b, 48);
                if pre > 0 {
                    off = inet6_opt_append(b, 48, off, 0x20, pre, 1, &raw mut data);
                }
                off = inet6_opt_append(b, 48, off, 0x21, 8, 8, &raw mut data);
                let end = inet6_opt_finish(b, 48, off);
                let got = format!(" {end}") + &hex(&buf[..end as usize]);
                assert_eq!(got, oracle(&format!("opt-pad {pre}")), "pre {pre}");
            }
        }
    }

    #[test]
    fn rth_is_glibcs() {
        unsafe {
            let got = [(0, 0), (0, 3), (0, 127), (0, 128), (2, 1)]
                .iter()
                .map(|&(t, s)| format!(" {}", inet6_rth_space(t, s)))
                .collect::<Vec<_>>()
                .concat();
            assert_eq!(got, oracle("rth-space"));
            let mut buf = [0xEEu8; 128];
            let b = buf.as_mut_ptr();
            let r = inet6_rth_init(b, 128, 0, 3);
            let mut got = at(b, r);
            let mut addrs = [[0u8; 16]; 4];
            for (i, a) in addrs.iter_mut().enumerate() {
                a[0] = 0x20;
                a[15] = i as u8 + 1;
            }
            for a in &addrs {
                got += &format!(" {}", inet6_rth_add(b, a.as_ptr()));
            }
            got += &format!(" segs={}", inet6_rth_segments(b));
            for i in -1..=3 {
                got += &at(b, inet6_rth_getaddr(b, i));
            }
            got += &hex(&buf[..56]);
            assert_eq!(got, oracle("rth-init"));
            let mut out = [0xEEu8; 128];
            let mut got = format!(" {}", inet6_rth_reverse(b, out.as_mut_ptr())) + &hex(&out[..56]);
            got += &(format!(" {}", inet6_rth_reverse(b, b)) + &hex(&buf[..56]));
            assert_eq!(got, oracle("rth-reverse"));
            let got = at(b, inet6_rth_init(b, 20, 0, 3))
                + &at(b, inet6_rth_init(b, 128, 2, 1))
                + &at(b, inet6_rth_init(b, 128, 0, 128));
            assert_eq!(got, oracle("rth-refused"));
        }
    }

    /// glibc's own RFC 2292 buffer, parsed: the same options, padding and
    /// all, but NULL at the end where glibc's leaves a pointer past it.
    #[test]
    fn option_next_and_find_read_glibcs_buffer() {
        let line = oracle("option-init");
        let bytes = unhex(line.rsplit_once('[').unwrap().1.trim_end_matches(']'));
        let mut aligned = [0u64; 32];
        let buf = aligned.as_mut_ptr().cast::<u8>();
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
            let mut t = null_mut();
            let mut got = String::new();
            while inet6_option_next(buf, &raw mut t) == 0 {
                got += &(at(buf, t) + &format!(":{:02x}", *t));
            }
            got += &at(buf, t);
            let glibc = oracle("option-next");
            let (glibc_steps, glibc_end) = glibc.rsplit_once(' ').unwrap();
            assert_eq!(got, format!("{glibc_steps} NULL"), "glibc ends {glibc_end}");
            let mut t = null_mut();
            let mut got = format!(" {}", inet6_option_find(buf, &raw mut t, 0x12)) + &at(buf, t);
            t = null_mut();
            got += &(format!(" {}", inet6_option_find(buf, &raw mut t, 0x99)) + &at(buf, t));
            assert_eq!(got, oracle("option-find"));
        }
        // The hand-built header: Pad1, an option, a PadN of no data.
        let mut aligned = [0u64; 8];
        let h = aligned.as_mut_ptr().cast::<u8>();
        unsafe {
            h.cast::<Cmsghdr>().write(Cmsghdr {
                len: CMSG_HEAD + 8,
                level: 41,
                typ: 54,
            });
            core::ptr::copy_nonoverlapping(
                [0x3B, 0, 0, 0x10, 1, 0xAA, 1, 0].as_ptr(),
                h.add(16),
                8,
            );
            let mut t = null_mut();
            let mut got = String::new();
            let mut r;
            loop {
                r = inet6_option_next(h, &raw mut t);
                if r != 0 {
                    break;
                }
                got += &(at(h, t) + &format!(":{:02x}", *t));
            }
            got += &(format!(" {r}") + &at(h, t));
            let glibc = oracle("option-next-pad1");
            let (glibc_steps, _) = glibc.rsplit_once(' ').unwrap();
            assert_eq!(got, format!("{glibc_steps} NULL"));
        }
    }

    /// RFC 2292 section 6.3.7's examples, built byte for byte -- with this
    /// machine's 16-byte `cmsghdr` where the RFC draws a 12-byte one.
    #[test]
    fn option_builds_the_rfcs_examples() {
        const X: u8 = 0x3C;
        const Y: u8 = 0x3D;
        let opt_x: [u8; 14] = [X, 12, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let opt_y: [u8; 9] = [Y, 7, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7];
        let mut aligned = [0u64; 32];
        let buf = aligned.as_mut_ptr().cast::<u8>();
        unsafe {
            let mut cmsg = null_mut();
            assert_eq!(inet6_option_init(buf, &raw mut cmsg, IPV6_HOPOPTS), 0);
            assert_eq!(cmsg, buf);
            // X (8n + 2): the Next Header and Hdr Ext Len bytes are its 2
            // bytes of padding.
            assert_eq!(inet6_option_append(cmsg, opt_x.as_ptr(), 8, 2), 0);
            let hdr = &*buf.cast::<Cmsghdr>();
            assert_eq!(
                hdr.len,
                CMSG_HEAD + 16,
                "the RFC's 28, less its 12, plus 16"
            );
            let ext = core::slice::from_raw_parts(buf.add(16), 16);
            assert_eq!(ext[..2], [0, 1], "Hdr Ext Len 1");
            assert_eq!(ext[2..16], opt_x);
            // Y (4n + 3): 3 bytes of PadN before it, 4 after.
            assert_eq!(inet6_option_append(cmsg, opt_y.as_ptr(), 4, 3), 0);
            let hdr = &*buf.cast::<Cmsghdr>();
            assert_eq!(hdr.len, CMSG_HEAD + 32, "the RFC's 44, less its 12");
            let ext = core::slice::from_raw_parts(buf.add(16), 32);
            assert_eq!(ext[1], 3, "Hdr Ext Len 3");
            assert_eq!(ext[16..19], [PADN, 1, 0]);
            assert_eq!(ext[19..28], opt_y);
            assert_eq!(ext[28..32], [PADN, 2, 0, 0]);
            // Y alone: a Pad1 before it, a PadN of 4 after.
            let mut aligned = [0u64; 32];
            let buf = aligned.as_mut_ptr().cast::<u8>();
            assert_eq!(inet6_option_init(buf, &raw mut cmsg, IPV6_DSTOPTS), 0);
            assert_eq!(inet6_option_append(cmsg, opt_y.as_ptr(), 4, 3), 0);
            let ext = core::slice::from_raw_parts(buf.add(16), 16);
            assert_eq!(ext[..3], [0, 1, PAD1]);
            assert_eq!(ext[3..12], opt_y);
            assert_eq!(ext[12..16], [PADN, 2, 0, 0]);
            // Walked back: X is not there; Y is, after the Pad1.
            let mut t = null_mut();
            assert_eq!(inet6_option_find(cmsg, &raw mut t, i32::from(Y)), 0);
            assert_eq!(t, buf.add(16 + 3));
            // alloc: room for the TLV of a 1-byte datum, 3 bytes.
            let p = inet6_option_alloc(cmsg, 1, 1, 0);
            assert_eq!(p, buf.add(16 + 12), "where the tail padding was");
            let hdr = &*buf.cast::<Cmsghdr>();
            assert_eq!(hdr.len, CMSG_HEAD + 16);
            let ext = core::slice::from_raw_parts(buf.add(16), 16);
            assert_eq!(ext[12..16], [0, 0, 0, PAD1], "zeroed, then a Pad1");
        }
        // The refusals and the space.
        unsafe {
            let mut aligned = [0u64; 8];
            let buf = aligned.as_mut_ptr().cast::<u8>();
            let mut cmsg = null_mut();
            assert_eq!(inet6_option_init(buf, &raw mut cmsg, 7), -1);
            assert_eq!(inet6_option_init(buf, &raw mut cmsg, IPV6_HOPOPTS), 0);
            assert_eq!(
                inet6_option_append(cmsg, [0u8, 0].as_ptr(), 1, 0),
                -1,
                "Pad1"
            );
            assert_eq!(inet6_option_append(cmsg, opt_y.as_ptr(), 3, 0), -1);
            assert!(inet6_option_alloc(cmsg, 3, 3, 0).is_null());
            assert!(inet6_option_alloc(cmsg, 3, 1, 8).is_null());
        }
        let got = [0, 1, 6, 20]
            .iter()
            .map(|&n| format!(" {}", inet6_option_space(n)))
            .collect::<Vec<_>>()
            .concat();
        assert_eq!(got, oracle("option-space"));
    }

    #[test]
    fn bindresvport_refuses_another_family() {
        let mut sin = crate::socket::SockaddrIn {
            sin_family: crate::socket::AF_INET6 as u16,
            sin_port: 0,
            sin_addr: crate::socket::InAddr { s_addr: 0 },
            sin_zero: [0; 8],
        };
        crate::errno::set_errno(0);
        assert_eq!(unsafe { bindresvport(-1, &raw mut sin) }, -1);
        assert_eq!(get_errno(), EAFNOSUPPORT);
        // A descriptor that is no socket: bind's error, not a search.
        assert_eq!(unsafe { bindresvport(-1, null_mut()) }, -1);
        assert_ne!(get_errno(), EADDRINUSE);
    }

    #[test]
    fn the_source_filters_are_refused() {
        crate::errno::set_errno(0);
        assert_eq!(
            getipv4sourcefilter(-1, 0, 0, null_mut(), null_mut(), null_mut()),
            -1
        );
        assert_eq!(get_errno(), crate::errno::EBADF);
        let group = [2u8, 0, 0, 0, 224, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            unsafe {
                getsourcefilter(-1, 0, group.as_ptr(), 3, null_mut(), null_mut(), null_mut())
            },
            -1
        );
        assert_eq!(get_errno(), EINVAL, "too short, as glibc's");
        assert_eq!(
            unsafe { setsourcefilter(-1, 0, group.as_ptr(), 16, 1, 0, null()) },
            -1
        );
        assert_eq!(get_errno(), crate::errno::EBADF);
    }
}
