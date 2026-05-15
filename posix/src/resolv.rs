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
// dn_expand
// ---------------------------------------------------------------------------

/// `dn_expand` — expand a compressed domain name.
///
/// Stub: always returns -1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dn_expand(
    _msg: *const u8,
    _eomorig: *const u8,
    _comp_dn: *const u8,
    _exp_dn: *mut u8,
    _length: i32,
) -> i32 {
    -1
}

// ---------------------------------------------------------------------------
// dn_comp
// ---------------------------------------------------------------------------

/// `dn_comp` — compress a domain name.
///
/// Stub: always returns -1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dn_comp(
    _exp_dn: *const u8,
    _comp_dn: *mut u8,
    _length: i32,
    _dnptrs: *mut *mut u8,
    _lastdnptr: *mut *mut u8,
) -> i32 {
    -1
}

// ---------------------------------------------------------------------------
// dn_skipname
// ---------------------------------------------------------------------------

/// `dn_skipname` — skip over a compressed domain name.
///
/// Stub: always returns -1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dn_skipname(
    _comp_dn: *const u8,
    _eom: *const u8,
) -> i32 {
    -1
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

    #[test]
    fn test_dn_expand_returns_neg1() {
        let mut out = [0u8; 256];
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
    fn test_dn_comp_returns_neg1() {
        let mut out = [0u8; 256];
        let ret = dn_comp(
            core::ptr::null(),
            out.as_mut_ptr(),
            out.len() as i32,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_dn_skipname_returns_neg1() {
        let ret = dn_skipname(core::ptr::null(), core::ptr::null());
        assert_eq!(ret, -1);
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
