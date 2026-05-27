//! `<resolv.h>` — DNS resolver stubs.
//!
//! Provides stubs for `res_init`, `res_query`, `res_search`,
//! `res_mkquery`, `res_send`, `dn_expand`, `dn_comp`, `dn_skipname`,
//! `ns_get16`, `ns_get32`, `ns_put16`, `ns_put32`.
//!
//! Our OS does not yet have a full DNS resolver stack.  These stubs
//! satisfy link-time references and return appropriate errors.
//! Programs needing DNS should use `getaddrinfo()` (in socket.rs),
//! which may be backed by a userspace resolver when available.

use crate::errno;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum DNS name length.
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
/// DNS type: ANY (any type).
pub const T_ANY: i32 = 255;

/// DNS operation: Standard query.
pub const QUERY: i32 = 0;
/// DNS operation: Inverse query.
pub const IQUERY: i32 = 1;

// ---------------------------------------------------------------------------
// res_init
// ---------------------------------------------------------------------------

/// `res_init` — initialize the resolver.
///
/// Stub: always returns 0 (success) but does nothing.
/// Per convention, `res_init` initializes the resolver state from
/// `/etc/resolv.conf`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_init() -> i32 {
    0
}

/// `__res_init` — glibc alias for `res_init`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __res_init() -> i32 {
    res_init()
}

// ---------------------------------------------------------------------------
// res_query / res_search
// ---------------------------------------------------------------------------

/// `res_query` — make a DNS query.
///
/// Stub: always fails with -1 and sets errno to ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_query(
    _dname: *const u8,
    _class: i32,
    _type_: i32,
    _answer: *mut u8,
    _anslen: i32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
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

/// `res_search` — search DNS with domain search list.
///
/// Stub: delegates to `res_query`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_search(
    dname: *const u8,
    class: i32,
    type_: i32,
    answer: *mut u8,
    anslen: i32,
) -> i32 {
    res_query(dname, class, type_, answer, anslen)
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
// res_mkquery
// ---------------------------------------------------------------------------

/// `res_mkquery` — construct a DNS query message.
///
/// Stub: always fails with -1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_mkquery(
    _op: i32,
    _dname: *const u8,
    _class: i32,
    _type_: i32,
    _data: *const u8,
    _datalen: i32,
    _newrr: *const u8,
    _buf: *mut u8,
    _buflen: i32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// res_send
// ---------------------------------------------------------------------------

/// `res_send` — send a pre-formatted DNS query.
///
/// Stub: always fails with -1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn res_send(
    _msg: *const u8,
    _msglen: i32,
    _answer: *mut u8,
    _anslen: i32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// DNS name wire format helpers
// ---------------------------------------------------------------------------
//
// Wire format:
//   each label is a length byte (0..=63) followed by that many label
//   bytes; a zero length byte terminates the name; a length byte with
//   the top two bits set (0b11_xxxxxx) is a compression pointer whose
//   low 6 bits plus the next byte give a 14-bit offset from the start
//   of the message.

/// Cap on the number of compression-pointer hops we follow in a single
/// expansion before declaring a malformed packet.  Each hop must
/// produce at least one label or the terminator, so 64 is plenty for
/// any well-formed name (max 255 octets total).
const MAX_DN_HOPS: usize = 64;

// ---------------------------------------------------------------------------
// dn_expand
// ---------------------------------------------------------------------------

/// `dn_expand` — expand a compressed domain name from a DNS packet.
///
/// `msg` points to the start of the packet, `eomorig` to its
/// one-past-the-end byte, `comp_dn` to the start of the compressed
/// name (somewhere within `[msg, eomorig)`), and `exp_dn` to a buffer
/// of `length` bytes that receives the dotted, null-terminated
/// expanded name.
///
/// The return value is the number of bytes consumed from `comp_dn`
/// *at its original position* — i.e. without following any compression
/// pointers, so the caller can advance past the encoded name in the
/// packet.  Returns -1 on malformed input or buffer overflow.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dn_expand(
    msg: *const u8,
    eomorig: *const u8,
    comp_dn: *const u8,
    exp_dn: *mut u8,
    length: i32,
) -> i32 {
    if msg.is_null() || eomorig.is_null() || comp_dn.is_null() || exp_dn.is_null() {
        return -1;
    }
    if length <= 0 {
        return -1;
    }
    let cap = length as usize;
    // SAFETY: caller contract — msg <= eomorig as range bounds.
    let msg_len: usize = unsafe { eomorig.offset_from(msg) } as usize;
    let start_off = match offset_of(msg, comp_dn, msg_len) {
        Some(o) => o,
        None => return -1,
    };

    let mut original_bytes: usize = 0; // bytes consumed at the start position
    let mut followed_pointer = false;
    let mut off = start_off;
    let mut out_pos: usize = 0;
    let mut hops: usize = 0;

    loop {
        if hops > MAX_DN_HOPS {
            return -1;
        }
        if off >= msg_len {
            return -1;
        }
        // SAFETY: msg + off is within [msg, eomorig).
        let b = unsafe { *msg.add(off) };
        if (b & 0xc0) == 0xc0 {
            // Pointer.
            if off.wrapping_add(1) >= msg_len {
                return -1;
            }
            let lo = unsafe { *msg.add(off.wrapping_add(1)) };
            let new_off = (((b & 0x3f) as usize) << 8) | (lo as usize);
            if !followed_pointer {
                original_bytes = off.wrapping_add(2).wrapping_sub(start_off);
                followed_pointer = true;
            }
            // Pointers must point backward in a well-formed packet (RFC
            // 1035): require strictly less than the current offset to
            // ensure progress.
            if new_off >= off {
                return -1;
            }
            off = new_off;
            hops = hops.wrapping_add(1);
            continue;
        }
        if (b & 0xc0) != 0 {
            // Reserved label type.
            return -1;
        }
        if b == 0 {
            // End of name.
            if !followed_pointer {
                original_bytes = off.wrapping_add(1).wrapping_sub(start_off);
            }
            // Write null terminator.
            if out_pos >= cap {
                return -1;
            }
            // SAFETY: out_pos < cap and exp_dn covers cap bytes.
            unsafe { *exp_dn.add(out_pos) = 0; }
            // If we emitted no labels (root name "."), still null-terminate.
            return original_bytes as i32;
        }
        let label_len = b as usize;
        if label_len > 63 {
            return -1;
        }
        if off.wrapping_add(1).wrapping_add(label_len) > msg_len {
            return -1;
        }
        // Emit '.' separator if this isn't the first label.
        if out_pos > 0 {
            if out_pos.wrapping_add(1) >= cap {
                return -1;
            }
            // SAFETY: index in bounds.
            unsafe { *exp_dn.add(out_pos) = b'.'; }
            out_pos = out_pos.wrapping_add(1);
        }
        // Emit label bytes.  Need room for label + later null terminator
        // (we'll check for that later when we finish; for now just
        // reserve at least one byte for the terminator).
        if out_pos.wrapping_add(label_len).wrapping_add(1) > cap {
            return -1;
        }
        let mut k: usize = 0;
        while k < label_len {
            // SAFETY: both pointers are within their respective buffers.
            let c = unsafe { *msg.add(off.wrapping_add(1).wrapping_add(k)) };
            unsafe { *exp_dn.add(out_pos.wrapping_add(k)) = c; }
            k = k.wrapping_add(1);
        }
        out_pos = out_pos.wrapping_add(label_len);
        off = off.wrapping_add(1).wrapping_add(label_len);
    }
}

/// Compute `target - base` in bytes if `target` is within
/// `[base, base + base_len]`, else None.  Avoids the wrap-around hazard
/// of `offset_from` when the pointers come from unrelated allocations.
fn offset_of(base: *const u8, target: *const u8, base_len: usize) -> Option<usize> {
    let b = base as usize;
    let t = target as usize;
    if t < b {
        return None;
    }
    let off = t.wrapping_sub(b);
    if off > base_len {
        return None;
    }
    Some(off)
}

// ---------------------------------------------------------------------------
// dn_skipname
// ---------------------------------------------------------------------------

/// `dn_skipname` — return the number of bytes a compressed domain name
/// occupies in the wire packet.
///
/// `comp_dn` points at the start of the name, `eom` at the one-past-end
/// byte of the packet.  Following compression pointers is *not*
/// required: a pointer counts as two bytes and stops the walk.
/// Returns -1 on malformed input.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dn_skipname(comp_dn: *const u8, eom: *const u8) -> i32 {
    if comp_dn.is_null() || eom.is_null() {
        return -1;
    }
    let avail = match offset_of(comp_dn, eom, usize::MAX) {
        Some(n) => n,
        None => return -1,
    };
    let mut off: usize = 0;
    loop {
        if off >= avail {
            return -1;
        }
        // SAFETY: off < avail and comp_dn covers avail bytes.
        let b = unsafe { *comp_dn.add(off) };
        if (b & 0xc0) == 0xc0 {
            // 2-byte pointer ends the name.
            if off.wrapping_add(2) > avail {
                return -1;
            }
            return off.wrapping_add(2) as i32;
        }
        if (b & 0xc0) != 0 {
            // Reserved label type.
            return -1;
        }
        if b == 0 {
            return off.wrapping_add(1) as i32;
        }
        let label_len = b as usize;
        if label_len > 63 {
            return -1;
        }
        off = off.wrapping_add(1).wrapping_add(label_len);
    }
}

// ---------------------------------------------------------------------------
// dn_comp
// ---------------------------------------------------------------------------

/// `dn_comp` — compress a dotted domain name into wire format.
///
/// `exp_dn` is a null-terminated dotted name (e.g. `"www.example.com\0"`).
/// `comp_dn` receives the wire-format encoding, which is at most
/// `length` bytes.  Returns the number of bytes written, or -1 on
/// error (invalid input or insufficient buffer).
///
/// `dnptrs` / `lastdnptr` describe a (caller-managed) table of pointers
/// to previously-emitted names within the same packet, used for
/// compression.  This implementation ignores them — DNS allows but
/// doesn't require compression, and emitting uncompressed names is
/// always correct.  When non-null, our implementation still respects
/// the convention that `*dnptrs == NULL` means "no entries yet" and
/// `dnptrs == NULL` itself means "no compression please".
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dn_comp(
    exp_dn: *const u8,
    comp_dn: *mut u8,
    length: i32,
    _dnptrs: *mut *mut u8,
    _lastdnptr: *mut *mut u8,
) -> i32 {
    if exp_dn.is_null() || comp_dn.is_null() || length <= 0 {
        return -1;
    }
    let cap = length as usize;
    if cap < 1 {
        return -1;
    }

    // Handle empty input ("") or root-only input ("."): emit a single
    // zero terminator (the root-domain wire encoding).
    // SAFETY: caller contract — exp_dn is null-terminated.
    let first = unsafe { *exp_dn };
    if first == 0 {
        unsafe { *comp_dn = 0; }
        return 1;
    }
    if first == b'.' {
        // SAFETY: previous byte was '.', not null, so checking the next
        // byte is bounded by the caller's null terminator at some point.
        let second = unsafe { *exp_dn.add(1) };
        if second == 0 {
            unsafe { *comp_dn = 0; }
            return 1;
        }
        // Otherwise it's a leading dot followed by more text — invalid.
        return -1;
    }

    let mut in_pos: usize = 0;
    let mut label_start: usize = 0;
    let mut label_len_pos: usize = 0;
    let mut out_pos: usize = 1; // reserve byte 0 for the first label's length

    loop {
        // SAFETY: caller contract — exp_dn is null-terminated.
        let b = unsafe { *exp_dn.add(in_pos) };
        if b == 0 || b == b'.' {
            let label_len = in_pos.wrapping_sub(label_start);
            if label_len == 0 {
                // Empty intermediate label ("a..b") or trailing-non-root
                // emptiness is invalid.  Trailing "." (e.g. "a.b.") at
                // the very end is allowed: detect it by peeking past.
                if b == b'.' {
                    return -1;
                }
                // b == 0 and label_len == 0: this is the legitimate
                // "trailing dot" case — we already advanced past a '.'
                // and now hit the null terminator.  Emit terminator
                // (no length byte needed since we already reserved one
                // we now have to undo).  We must back off out_pos by 1.
                out_pos = out_pos.wrapping_sub(1);
                // SAFETY: out_pos < cap (we checked before reserving).
                unsafe { *comp_dn.add(out_pos) = 0; }
                return out_pos.wrapping_add(1) as i32;
            }
            if label_len > 63 {
                return -1;
            }
            // SAFETY: label_len_pos < cap because we reserved it.
            unsafe { *comp_dn.add(label_len_pos) = label_len as u8; }
            if b == 0 {
                // Final terminator.
                if out_pos.wrapping_add(1) > cap {
                    return -1;
                }
                // SAFETY: out_pos < cap.
                unsafe { *comp_dn.add(out_pos) = 0; }
                return out_pos.wrapping_add(1) as i32;
            }
            // b == '.': advance to next label; reserve its length byte.
            in_pos = in_pos.wrapping_add(1);
            label_start = in_pos;
            label_len_pos = out_pos;
            if out_pos.wrapping_add(1) > cap {
                return -1;
            }
            out_pos = out_pos.wrapping_add(1);
            continue;
        }
        // Copy the character into the label body.
        if out_pos.wrapping_add(1) > cap {
            return -1;
        }
        // SAFETY: out_pos < cap by the check above.
        unsafe { *comp_dn.add(out_pos) = b; }
        out_pos = out_pos.wrapping_add(1);
        in_pos = in_pos.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// ns_get16 / ns_get32 / ns_put16 / ns_put32
// ---------------------------------------------------------------------------

/// `ns_get16` — get a 16-bit value from network byte order.
///
/// Reads a big-endian 16-bit value from the buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_get16(src: *const u8) -> u16 {
    if src.is_null() {
        return 0;
    }
    // SAFETY: caller guarantees at least 2 bytes.
    let b0 = unsafe { *src } as u16;
    let b1 = unsafe { *src.add(1) } as u16;
    (b0 << 8) | b1
}

/// `ns_get32` — get a 32-bit value from network byte order.
///
/// Reads a big-endian 32-bit value from the buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_get32(src: *const u8) -> u32 {
    if src.is_null() {
        return 0;
    }
    // SAFETY: caller guarantees at least 4 bytes.
    let b0 = unsafe { *src } as u32;
    let b1 = unsafe { *src.add(1) } as u32;
    let b2 = unsafe { *src.add(2) } as u32;
    let b3 = unsafe { *src.add(3) } as u32;
    (b0 << 24) | (b1 << 16) | (b2 << 8) | b3
}

/// `ns_put16` — put a 16-bit value in network byte order.
///
/// Writes a big-endian 16-bit value to the buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_put16(val: u16, dst: *mut u8) {
    if dst.is_null() {
        return;
    }
    // SAFETY: caller guarantees at least 2 bytes.
    unsafe {
        *dst = (val >> 8) as u8;
        *dst.add(1) = val as u8;
    }
}

/// `ns_put32` — put a 32-bit value in network byte order.
///
/// Writes a big-endian 32-bit value to the buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_put32(val: u32, dst: *mut u8) {
    if dst.is_null() {
        return;
    }
    // SAFETY: caller guarantees at least 4 bytes.
    unsafe {
        *dst = (val >> 24) as u8;
        *dst.add(1) = (val >> 16) as u8;
        *dst.add(2) = (val >> 8) as u8;
        *dst.add(3) = val as u8;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_dns_name_limits() {
        assert_eq!(MAXDNAME, 1025);
        assert_eq!(MAXCDNAME, 255);
        assert_eq!(MAXLABEL, 63);
    }

    #[test]
    fn test_dns_sizes() {
        assert_eq!(HFIXEDSZ, 12);
        assert_eq!(QFIXEDSZ, 4);
        assert_eq!(RRFIXEDSZ, 10);
    }

    #[test]
    fn test_dns_classes() {
        assert_eq!(C_IN, 1);
        assert_eq!(C_CH, 3);
        assert_eq!(C_ANY, 255);
    }

    #[test]
    fn test_dns_types() {
        assert_eq!(T_A, 1);
        assert_eq!(T_AAAA, 28);
        assert_eq!(T_CNAME, 5);
        assert_eq!(T_MX, 15);
        assert_eq!(T_NS, 2);
        assert_eq!(T_PTR, 12);
        assert_eq!(T_SOA, 6);
        assert_eq!(T_SRV, 33);
        assert_eq!(T_TXT, 16);
        assert_eq!(T_ANY, 255);
    }

    #[test]
    fn test_dns_operations() {
        assert_eq!(QUERY, 0);
        assert_eq!(IQUERY, 1);
    }

    // -----------------------------------------------------------------------
    // res_init
    // -----------------------------------------------------------------------

    #[test]
    fn test_res_init_returns_zero() {
        assert_eq!(res_init(), 0);
    }

    #[test]
    fn test_res_init_alias() {
        assert_eq!(__res_init(), 0);
    }

    #[test]
    fn test_res_init_multiple_calls() {
        // Should be safe to call multiple times.
        for _ in 0..5 {
            assert_eq!(res_init(), 0);
        }
    }

    // -----------------------------------------------------------------------
    // res_query
    // -----------------------------------------------------------------------

    #[test]
    fn test_res_query_enosys() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 512];
        let ret = res_query(
            b"example.com\0".as_ptr(),
            C_IN,
            T_A,
            buf.as_mut_ptr(),
            buf.len() as i32,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_res_query_null_dname() {
        let mut buf = [0u8; 512];
        let ret = res_query(
            core::ptr::null(),
            C_IN,
            T_A,
            buf.as_mut_ptr(),
            buf.len() as i32,
        );
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_res_query_alias() {
        let mut buf = [0u8; 64];
        let ret = __res_query(
            b"test\0".as_ptr(),
            C_IN,
            T_AAAA,
            buf.as_mut_ptr(),
            buf.len() as i32,
        );
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // res_search
    // -----------------------------------------------------------------------

    #[test]
    fn test_res_search_enosys() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 512];
        let ret = res_search(
            b"host\0".as_ptr(),
            C_IN,
            T_A,
            buf.as_mut_ptr(),
            buf.len() as i32,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_res_search_alias() {
        let mut buf = [0u8; 64];
        let ret = __res_search(
            b"x\0".as_ptr(),
            C_IN,
            T_A,
            buf.as_mut_ptr(),
            buf.len() as i32,
        );
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // res_mkquery
    // -----------------------------------------------------------------------

    #[test]
    fn test_res_mkquery_enosys() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 512];
        let ret = res_mkquery(
            QUERY,
            b"example.com\0".as_ptr(),
            C_IN,
            T_A,
            core::ptr::null(),
            0,
            core::ptr::null(),
            buf.as_mut_ptr(),
            buf.len() as i32,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // res_send
    // -----------------------------------------------------------------------

    #[test]
    fn test_res_send_enosys() {
        crate::errno::set_errno(0);
        let query = [0u8; 32];
        let mut answer = [0u8; 512];
        let ret = res_send(
            query.as_ptr(),
            query.len() as i32,
            answer.as_mut_ptr(),
            answer.len() as i32,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // dn_expand / dn_comp / dn_skipname
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // dn_comp / dn_expand / dn_skipname
    // -----------------------------------------------------------------------

    /// Helper: invoke dn_comp on a dotted C-string.
    fn comp(s: &[u8], out: &mut [u8]) -> i32 {
        // s must be null-terminated.
        assert!(s.last() == Some(&0), "input must be null-terminated");
        dn_comp(
            s.as_ptr(),
            out.as_mut_ptr(),
            out.len() as i32,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    }

    /// Helper: invoke dn_expand on a packet, starting at the given offset.
    fn expand(packet: &[u8], start: usize, out: &mut [u8]) -> i32 {
        let msg = packet.as_ptr();
        // SAFETY: msg + packet.len() is one-past-end of the slice.
        let eom = unsafe { msg.add(packet.len()) };
        // SAFETY: msg + start is within [msg, eom].
        let comp_dn = unsafe { msg.add(start) };
        dn_expand(msg, eom, comp_dn, out.as_mut_ptr(), out.len() as i32)
    }

    #[test]
    fn test_dn_comp_null_inputs_return_neg1() {
        let mut out = [0u8; 64];
        assert_eq!(
            dn_comp(
                core::ptr::null(),
                out.as_mut_ptr(),
                out.len() as i32,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            ),
            -1
        );
        let name = b"x\0";
        assert_eq!(
            dn_comp(
                name.as_ptr(),
                core::ptr::null_mut(),
                64,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            ),
            -1
        );
    }

    #[test]
    fn test_dn_comp_simple_name() {
        let mut out = [0u8; 64];
        let n = comp(b"x\0", &mut out);
        assert_eq!(n, 3);
        assert_eq!(&out[..3], &[1, b'x', 0]);
    }

    #[test]
    fn test_dn_comp_multi_label() {
        let mut out = [0u8; 64];
        let n = comp(b"www.example.com\0", &mut out);
        assert_eq!(n, 17);
        assert_eq!(
            &out[..17],
            &[3, b'w', b'w', b'w', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0]
        );
    }

    #[test]
    fn test_dn_comp_trailing_dot() {
        // "a.b." should encode the same as "a.b": 5 bytes.
        let mut out = [0u8; 64];
        let n = comp(b"a.b.\0", &mut out);
        assert_eq!(n, 5);
        assert_eq!(&out[..5], &[1, b'a', 1, b'b', 0]);
    }

    #[test]
    fn test_dn_comp_root_dot() {
        // "." is the root domain, encoded as a single zero byte.
        let mut out = [0u8; 64];
        let n = comp(b".\0", &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0], 0);
    }

    #[test]
    fn test_dn_comp_empty_input() {
        // Empty input ("") encodes the same as the root domain.
        let mut out = [0u8; 64];
        let n = comp(b"\0", &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0], 0);
    }

    #[test]
    fn test_dn_comp_double_dot_rejected() {
        // Empty intermediate labels are invalid.
        let mut out = [0u8; 64];
        let n = comp(b"a..b\0", &mut out);
        assert_eq!(n, -1);
    }

    #[test]
    fn test_dn_comp_leading_dot_rejected() {
        // Leading '.' followed by more text is invalid.
        let mut out = [0u8; 64];
        let n = comp(b".a\0", &mut out);
        assert_eq!(n, -1);
    }

    #[test]
    fn test_dn_comp_buffer_too_small() {
        // "abc" needs 5 bytes; give it 4.
        let mut out = [0u8; 4];
        let n = comp(b"abc\0", &mut out);
        assert_eq!(n, -1);
    }

    #[test]
    fn test_dn_comp_label_too_long() {
        // A 64-byte label is invalid (max is 63).
        let mut input = [b'a'; 65];
        input[64] = 0;
        let mut out = [0u8; 128];
        let n = comp(&input, &mut out);
        assert_eq!(n, -1);
    }

    #[test]
    fn test_dn_comp_max_label_ok() {
        // 63-byte labels are at the limit but still valid.
        let mut input = [b'a'; 64];
        input[63] = 0;
        let mut out = [0u8; 128];
        let n = comp(&input, &mut out);
        assert_eq!(n, 65);
        assert_eq!(out[0], 63);
        assert_eq!(out[64], 0);
    }

    #[test]
    fn test_dn_expand_null_inputs_return_neg1() {
        let mut out = [0u8; 64];
        let ret = dn_expand(
            core::ptr::null(),
            core::ptr::null(),
            core::ptr::null(),
            out.as_mut_ptr(),
            out.len() as i32,
        );
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_dn_expand_uncompressed_roundtrip() {
        // Build a packet that is just a single uncompressed name.
        let mut packet = [0u8; 32];
        let n = comp(b"www.example.com\0", &mut packet);
        assert_eq!(n, 17);
        let mut out = [0u8; 64];
        let ret = expand(&packet[..n as usize], 0, &mut out);
        assert_eq!(ret, 17);
        // Output must be a null-terminated "www.example.com".
        let z = out.iter().position(|&b| b == 0).unwrap();
        assert_eq!(&out[..z], b"www.example.com");
    }

    #[test]
    fn test_dn_expand_single_label_roundtrip() {
        let mut packet = [0u8; 16];
        let n = comp(b"abc\0", &mut packet);
        assert_eq!(n, 5);
        let mut out = [0u8; 32];
        let ret = expand(&packet[..n as usize], 0, &mut out);
        assert_eq!(ret, 5);
        let z = out.iter().position(|&b| b == 0).unwrap();
        assert_eq!(&out[..z], b"abc");
    }

    #[test]
    fn test_dn_expand_root_roundtrip() {
        // Root domain is a single zero byte.
        let packet = [0u8; 1];
        let mut out = [0u8; 16];
        let ret = expand(&packet, 0, &mut out);
        assert_eq!(ret, 1);
        // Empty dotted name => first byte of out is null.
        assert_eq!(out[0], 0);
    }

    #[test]
    fn test_dn_expand_follows_pointer() {
        // Packet layout:
        //   off 0: [3, 'a', 'b', 'c', 0]   uncompressed name
        //   off 5: [0xc0, 0x00]            pointer back to offset 0
        let packet = [3u8, b'a', b'b', b'c', 0, 0xc0, 0x00];
        let mut out = [0u8; 32];
        let ret = expand(&packet, 5, &mut out);
        assert_eq!(ret, 2); // pointer occupies 2 bytes at original position
        let z = out.iter().position(|&b| b == 0).unwrap();
        assert_eq!(&out[..z], b"abc");
    }

    #[test]
    fn test_dn_expand_chained_pointers() {
        // Pointers must point strictly backward.
        //   off 0: [1, 'x', 0]
        //   off 3: [1, 'y', 0xc0, 0x00]    name = "y.x" via pointer
        //   off 7: [0xc0, 0x03]            pointer to off 3
        let packet = [1u8, b'x', 0, 1, b'y', 0xc0, 0x00, 0xc0, 0x03];
        let mut out = [0u8; 32];
        let ret = expand(&packet, 7, &mut out);
        assert_eq!(ret, 2);
        let z = out.iter().position(|&b| b == 0).unwrap();
        assert_eq!(&out[..z], b"y.x");
    }

    #[test]
    fn test_dn_expand_forward_pointer_rejected() {
        // Pointer pointing forward (or to itself) is malformed.
        let packet = [0xc0u8, 0x02, 1, b'x', 0];
        let mut out = [0u8; 32];
        let ret = expand(&packet, 0, &mut out);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_dn_expand_self_pointer_rejected() {
        let packet = [0xc0u8, 0x00];
        let mut out = [0u8; 32];
        let ret = expand(&packet, 0, &mut out);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_dn_expand_reserved_label_type_rejected() {
        // 0x40 = 0b01_xxxxxx, 0x80 = 0b10_xxxxxx are reserved.
        for &bad in &[0x40u8, 0x80u8] {
            let packet = [bad, 0, 0];
            let mut out = [0u8; 32];
            let ret = expand(&packet, 0, &mut out);
            assert_eq!(ret, -1);
        }
    }

    #[test]
    fn test_dn_expand_oversize_label_rejected() {
        // Length 64 is invalid (max label is 63).
        // 64 = 0x40 which is actually a reserved type, so use a length that
        // claims to extend past the packet instead.
        let packet = [5u8, b'a', b'b', 0]; // claims 5 bytes but only 2 available
        let mut out = [0u8; 32];
        let ret = expand(&packet, 0, &mut out);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_dn_expand_missing_terminator_rejected() {
        // No zero terminator and no pointer.
        let packet = [3u8, b'a', b'b', b'c'];
        let mut out = [0u8; 32];
        let ret = expand(&packet, 0, &mut out);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_dn_expand_output_buffer_too_small() {
        let mut packet = [0u8; 32];
        let n = comp(b"www.example.com\0", &mut packet);
        // Need 16 bytes for "www.example.com\0", give it 8.
        let mut out = [0u8; 8];
        let ret = expand(&packet[..n as usize], 0, &mut out);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_dn_skipname_null_inputs() {
        assert_eq!(dn_skipname(core::ptr::null(), core::ptr::null()), -1);
    }

    #[test]
    fn test_dn_skipname_simple_name() {
        let packet = [3u8, b'a', b'b', b'c', 0];
        // SAFETY: pointer arithmetic within the slice.
        let eom = unsafe { packet.as_ptr().add(packet.len()) };
        let n = dn_skipname(packet.as_ptr(), eom);
        assert_eq!(n, 5);
    }

    #[test]
    fn test_dn_skipname_pointer_counts_as_2() {
        let packet = [0xc0u8, 0x00];
        let eom = unsafe { packet.as_ptr().add(packet.len()) };
        let n = dn_skipname(packet.as_ptr(), eom);
        assert_eq!(n, 2);
    }

    #[test]
    fn test_dn_skipname_label_then_pointer() {
        // [1, 'a', 0xc0, 0x00]: 1-byte length + 1-byte data + 2-byte pointer
        let packet = [1u8, b'a', 0xc0, 0x00];
        let eom = unsafe { packet.as_ptr().add(packet.len()) };
        let n = dn_skipname(packet.as_ptr(), eom);
        assert_eq!(n, 4);
    }

    #[test]
    fn test_dn_skipname_truncated_rejected() {
        // 2-byte payload claimed but only 1 byte available.
        let packet = [2u8, b'a'];
        let eom = unsafe { packet.as_ptr().add(packet.len()) };
        let n = dn_skipname(packet.as_ptr(), eom);
        assert_eq!(n, -1);
    }

    #[test]
    fn test_dn_skipname_reserved_label_rejected() {
        let packet = [0x40u8, 0];
        let eom = unsafe { packet.as_ptr().add(packet.len()) };
        let n = dn_skipname(packet.as_ptr(), eom);
        assert_eq!(n, -1);
    }

    #[test]
    fn test_dn_skipname_missing_terminator_rejected() {
        let packet = [3u8, b'a', b'b', b'c'];
        let eom = unsafe { packet.as_ptr().add(packet.len()) };
        let n = dn_skipname(packet.as_ptr(), eom);
        assert_eq!(n, -1);
    }

    // -----------------------------------------------------------------------
    // ns_get16 / ns_get32
    // -----------------------------------------------------------------------

    #[test]
    fn test_ns_get16_basic() {
        let buf = [0x01u8, 0x00]; // big-endian 256
        assert_eq!(ns_get16(buf.as_ptr()), 256);
    }

    #[test]
    fn test_ns_get16_zero() {
        let buf = [0x00u8, 0x00];
        assert_eq!(ns_get16(buf.as_ptr()), 0);
    }

    #[test]
    fn test_ns_get16_max() {
        let buf = [0xFFu8, 0xFF];
        assert_eq!(ns_get16(buf.as_ptr()), 0xFFFF);
    }

    #[test]
    fn test_ns_get16_null() {
        assert_eq!(ns_get16(core::ptr::null()), 0);
    }

    #[test]
    fn test_ns_get32_basic() {
        let buf = [0x00u8, 0x00, 0x01, 0x00]; // big-endian 256
        assert_eq!(ns_get32(buf.as_ptr()), 256);
    }

    #[test]
    fn test_ns_get32_large() {
        let buf = [0x7Fu8, 0xFF, 0xFF, 0xFF]; // big-endian 2147483647
        assert_eq!(ns_get32(buf.as_ptr()), 0x7FFFFFFF);
    }

    #[test]
    fn test_ns_get32_null() {
        assert_eq!(ns_get32(core::ptr::null()), 0);
    }

    // -----------------------------------------------------------------------
    // ns_put16 / ns_put32
    // -----------------------------------------------------------------------

    #[test]
    fn test_ns_put16_basic() {
        let mut buf = [0u8; 2];
        ns_put16(256, buf.as_mut_ptr());
        assert_eq!(buf, [0x01, 0x00]);
    }

    #[test]
    fn test_ns_put16_zero() {
        let mut buf = [0xFFu8; 2];
        ns_put16(0, buf.as_mut_ptr());
        assert_eq!(buf, [0x00, 0x00]);
    }

    #[test]
    fn test_ns_put16_max() {
        let mut buf = [0u8; 2];
        ns_put16(0xFFFF, buf.as_mut_ptr());
        assert_eq!(buf, [0xFF, 0xFF]);
    }

    #[test]
    fn test_ns_put16_null_no_crash() {
        ns_put16(42, core::ptr::null_mut());
    }

    #[test]
    fn test_ns_put32_basic() {
        let mut buf = [0u8; 4];
        ns_put32(256, buf.as_mut_ptr());
        assert_eq!(buf, [0x00, 0x00, 0x01, 0x00]);
    }

    #[test]
    fn test_ns_put32_max() {
        let mut buf = [0u8; 4];
        ns_put32(0xFFFFFFFF, buf.as_mut_ptr());
        assert_eq!(buf, [0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_ns_put32_null_no_crash() {
        ns_put32(42, core::ptr::null_mut());
    }

    // -----------------------------------------------------------------------
    // ns_get / ns_put roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn test_ns_16_roundtrip() {
        for val in [0u16, 1, 255, 256, 1000, 0x1234, 0xFFFF] {
            let mut buf = [0u8; 2];
            ns_put16(val, buf.as_mut_ptr());
            assert_eq!(ns_get16(buf.as_ptr()), val, "roundtrip failed for {val}");
        }
    }

    #[test]
    fn test_ns_32_roundtrip() {
        for val in [0u32, 1, 0xFF, 0x100, 0x12345678, 0xDEADBEEF, 0xFFFFFFFF] {
            let mut buf = [0u8; 4];
            ns_put32(val, buf.as_mut_ptr());
            assert_eq!(ns_get32(buf.as_ptr()), val, "roundtrip failed for {val}");
        }
    }

    // -----------------------------------------------------------------------
    // Workflow
    // -----------------------------------------------------------------------

    #[test]
    fn test_init_then_query_workflow() {
        // Typical usage: init → query.
        let ret = res_init();
        assert_eq!(ret, 0);

        let mut buf = [0u8; 512];
        let qret = res_query(
            b"example.com\0".as_ptr(),
            C_IN,
            T_A,
            buf.as_mut_ptr(),
            buf.len() as i32,
        );
        assert_eq!(qret, -1); // query fails (no resolver)
    }
}
