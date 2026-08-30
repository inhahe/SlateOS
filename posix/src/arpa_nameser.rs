//! `<arpa/nameser.h>` — DNS nameserver definitions.
//!
//! Provides constants for DNS message parsing and construction:
//! opcodes, response codes, record types, classes, and header
//! structure definitions.

// ---------------------------------------------------------------------------
// DNS header constants
// ---------------------------------------------------------------------------

/// Maximum size of a DNS packet.
pub const NS_PACKETSZ: usize = 512;

/// Maximum size of a DNS name (fully qualified, with dots and null).
pub const NS_MAXDNAME: usize = 1025;

/// Maximum number of labels in a DNS name.
pub const NS_MAXLABEL: usize = 63;

/// Maximum size of a compressed DNS name in a message.
pub const NS_MAXCDNAME: usize = 255;

/// Maximum size of a DNS message.
pub const NS_MAXMSG: usize = 65535;

/// Size of the fixed DNS header (12 bytes).
pub const NS_HFIXEDSZ: usize = 12;

/// Size of a fixed-length resource record (10 bytes: type + class +
/// TTL + rdlength).
pub const NS_RRFIXEDSZ: usize = 10;

/// Size of a query section entry (4 bytes: type + class).
pub const NS_QFIXEDSZ: usize = 4;

/// Port number for DNS.
pub const NS_DEFAULTPORT: u16 = 53;

// ---------------------------------------------------------------------------
// DNS opcodes
// ---------------------------------------------------------------------------

/// Standard query (QUERY).
pub const NS_O_QUERY: i32 = 0;

/// Inverse query (IQUERY, obsolete).
pub const NS_O_IQUERY: i32 = 1;

/// Server status request (STATUS).
pub const NS_O_STATUS: i32 = 2;

/// Notify (RFC 1996).
pub const NS_O_NOTIFY: i32 = 4;

/// Dynamic update (RFC 2136).
pub const NS_O_UPDATE: i32 = 5;

// ---------------------------------------------------------------------------
// DNS response codes (RCODE)
// ---------------------------------------------------------------------------

/// No error.
pub const NS_R_NOERROR: i32 = 0;

/// Format error.
pub const NS_R_FORMERR: i32 = 1;

/// Server failure.
pub const NS_R_SERVFAIL: i32 = 2;

/// Name error (NXDOMAIN).
pub const NS_R_NXDOMAIN: i32 = 3;

/// Not implemented.
pub const NS_R_NOTIMPL: i32 = 4;

/// Refused.
pub const NS_R_REFUSED: i32 = 5;

/// Name exists when it should not (YXDOMAIN).
pub const NS_R_YXDOMAIN: i32 = 6;

/// RR set exists when it should not (YXRRSET).
pub const NS_R_YXRRSET: i32 = 7;

/// RR set does not exist (NXRRSET).
pub const NS_R_NXRRSET: i32 = 8;

/// Not authoritative (NOTAUTH).
pub const NS_R_NOTAUTH: i32 = 9;

/// Name not in zone (NOTZONE).
pub const NS_R_NOTZONE: i32 = 10;

// ---------------------------------------------------------------------------
// DNS record types
// ---------------------------------------------------------------------------

/// Host address (A record).
pub const NS_T_A: u16 = 1;

/// Authoritative name server (NS record).
pub const NS_T_NS: u16 = 2;

/// Canonical name (CNAME record).
pub const NS_T_CNAME: u16 = 5;

/// Start of authority (SOA record).
pub const NS_T_SOA: u16 = 6;

/// Well-known service (WKS record).
pub const NS_T_WKS: u16 = 11;

/// Domain name pointer (PTR record).
pub const NS_T_PTR: u16 = 12;

/// Host information (HINFO record).
pub const NS_T_HINFO: u16 = 13;

/// Mail exchange (MX record).
pub const NS_T_MX: u16 = 15;

/// Text string (TXT record).
pub const NS_T_TXT: u16 = 16;

/// Responsible person (RP record).
pub const NS_T_RP: u16 = 17;

/// AFS database (AFSDB record).
pub const NS_T_AFSDB: u16 = 18;

/// IPv6 host address (AAAA record).
pub const NS_T_AAAA: u16 = 28;

/// Location (LOC record).
pub const NS_T_LOC: u16 = 29;

/// Server selection (SRV record).
pub const NS_T_SRV: u16 = 33;

/// Naming authority pointer (NAPTR record).
pub const NS_T_NAPTR: u16 = 35;

/// DNAME record (RFC 6672).
pub const NS_T_DNAME: u16 = 39;

/// Option (EDNS0, OPT pseudo-record).
pub const NS_T_OPT: u16 = 41;

/// DNSSEC delegation signer (DS record).
pub const NS_T_DS: u16 = 43;

/// DNSSEC signature (RRSIG record).
pub const NS_T_RRSIG: u16 = 46;

/// DNSSEC next secure (NSEC record).
pub const NS_T_NSEC: u16 = 47;

/// DNSSEC key (DNSKEY record).
pub const NS_T_DNSKEY: u16 = 48;

/// Certificate Association (TLSA record).
pub const NS_T_TLSA: u16 = 52;

/// Canonical name for DNAME (HTTPS/SVCB).
pub const NS_T_SVCB: u16 = 64;

/// HTTPS service binding.
pub const NS_T_HTTPS: u16 = 65;

/// Incremental zone transfer (IXFR).
pub const NS_T_IXFR: u16 = 251;

/// Full zone transfer (AXFR).
pub const NS_T_AXFR: u16 = 252;

/// All records (ANY query).
pub const NS_T_ANY: u16 = 255;

// ---------------------------------------------------------------------------
// DNS classes
// ---------------------------------------------------------------------------

/// Internet (IN).
pub const NS_C_IN: u16 = 1;

/// CSNET (obsolete).
pub const NS_C_CS: u16 = 2;

/// CHAOS.
pub const NS_C_CH: u16 = 3;

/// Hesiod.
pub const NS_C_HS: u16 = 4;

/// Any class (wildcard).
pub const NS_C_ANY: u16 = 255;

// ---------------------------------------------------------------------------
// Legacy BIND4 compatibility names
// ---------------------------------------------------------------------------

/// Alias for `NS_PACKETSZ`.
pub const PACKETSZ: usize = NS_PACKETSZ;

/// Alias for `NS_MAXDNAME`.
pub const MAXDNAME: usize = NS_MAXDNAME;

/// Alias for `NS_MAXLABEL`.
pub const MAXLABEL: usize = NS_MAXLABEL;

/// Alias for `NS_MAXCDNAME`.
pub const MAXCDNAME: usize = NS_MAXCDNAME;

/// Alias for `NS_HFIXEDSZ`.
pub const HFIXEDSZ: usize = NS_HFIXEDSZ;

/// Alias for `NS_RRFIXEDSZ`.
pub const RRFIXEDSZ: usize = NS_RRFIXEDSZ;

/// Alias for `NS_QFIXEDSZ`.
pub const QFIXEDSZ: usize = NS_QFIXEDSZ;

// Legacy record type names (T_A, T_NS, etc.)
/// A record.
pub const T_A: u16 = NS_T_A;
/// NS record.
pub const T_NS: u16 = NS_T_NS;
/// CNAME record.
pub const T_CNAME: u16 = NS_T_CNAME;
/// SOA record.
pub const T_SOA: u16 = NS_T_SOA;
/// PTR record.
pub const T_PTR: u16 = NS_T_PTR;
/// MX record.
pub const T_MX: u16 = NS_T_MX;
/// TXT record.
pub const T_TXT: u16 = NS_T_TXT;
/// AAAA record.
pub const T_AAAA: u16 = NS_T_AAAA;
/// SRV record.
pub const T_SRV: u16 = NS_T_SRV;
/// ANY query.
pub const T_ANY: u16 = NS_T_ANY;

// Legacy class names (C_IN, C_ANY)
/// Internet class.
pub const C_IN: u16 = NS_C_IN;
/// Any class.
pub const C_ANY: u16 = NS_C_ANY;

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Extract a 16-bit value from a network-byte-order buffer.
///
/// # Safety
/// `src` must point to at least 2 readable bytes.
#[inline]
pub unsafe fn ns_get16(src: *const u8) -> u16 {
    let hi = u16::from(unsafe { *src });
    let lo = u16::from(unsafe { *src.add(1) });
    (hi << 8) | lo
}

/// Extract a 32-bit value from a network-byte-order buffer.
///
/// # Safety
/// `src` must point to at least 4 readable bytes.
#[inline]
pub unsafe fn ns_get32(src: *const u8) -> u32 {
    let b0 = u32::from(unsafe { *src });
    let b1 = u32::from(unsafe { *src.add(1) });
    let b2 = u32::from(unsafe { *src.add(2) });
    let b3 = u32::from(unsafe { *src.add(3) });
    (b0 << 24) | (b1 << 16) | (b2 << 8) | b3
}

/// Store a 16-bit value in network byte order.
///
/// # Safety
/// `dst` must point to at least 2 writable bytes.
#[inline]
pub unsafe fn ns_put16(val: u16, dst: *mut u8) {
    unsafe {
        *dst = (val >> 8) as u8;
        *dst.add(1) = val as u8;
    }
}

/// Store a 32-bit value in network byte order.
///
/// # Safety
/// `dst` must point to at least 4 writable bytes.
#[inline]
pub unsafe fn ns_put32(val: u32, dst: *mut u8) {
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
    // Size constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_packetsz() {
        assert_eq!(NS_PACKETSZ, 512);
        assert_eq!(PACKETSZ, NS_PACKETSZ);
    }

    #[test]
    fn test_maxdname() {
        assert_eq!(NS_MAXDNAME, 1025);
        assert_eq!(MAXDNAME, NS_MAXDNAME);
    }

    #[test]
    fn test_hfixedsz() {
        assert_eq!(NS_HFIXEDSZ, 12);
        assert_eq!(HFIXEDSZ, NS_HFIXEDSZ);
    }

    #[test]
    fn test_rrfixedsz() {
        assert_eq!(NS_RRFIXEDSZ, 10);
        assert_eq!(RRFIXEDSZ, NS_RRFIXEDSZ);
    }

    #[test]
    fn test_qfixedsz() {
        assert_eq!(NS_QFIXEDSZ, 4);
        assert_eq!(QFIXEDSZ, NS_QFIXEDSZ);
    }

    #[test]
    fn test_defaultport() {
        assert_eq!(NS_DEFAULTPORT, 53);
    }

    // -----------------------------------------------------------------------
    // Opcodes
    // -----------------------------------------------------------------------

    #[test]
    fn test_opcodes() {
        assert_eq!(NS_O_QUERY, 0);
        assert_eq!(NS_O_IQUERY, 1);
        assert_eq!(NS_O_STATUS, 2);
        assert_eq!(NS_O_NOTIFY, 4);
        assert_eq!(NS_O_UPDATE, 5);
    }

    #[test]
    fn test_opcodes_distinct() {
        let ops = [
            NS_O_QUERY,
            NS_O_IQUERY,
            NS_O_STATUS,
            NS_O_NOTIFY,
            NS_O_UPDATE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Response codes
    // -----------------------------------------------------------------------

    #[test]
    fn test_rcodes() {
        assert_eq!(NS_R_NOERROR, 0);
        assert_eq!(NS_R_FORMERR, 1);
        assert_eq!(NS_R_SERVFAIL, 2);
        assert_eq!(NS_R_NXDOMAIN, 3);
        assert_eq!(NS_R_REFUSED, 5);
    }

    #[test]
    fn test_rcodes_distinct() {
        let rcodes = [
            NS_R_NOERROR,
            NS_R_FORMERR,
            NS_R_SERVFAIL,
            NS_R_NXDOMAIN,
            NS_R_NOTIMPL,
            NS_R_REFUSED,
            NS_R_YXDOMAIN,
            NS_R_YXRRSET,
            NS_R_NXRRSET,
            NS_R_NOTAUTH,
            NS_R_NOTZONE,
        ];
        for i in 0..rcodes.len() {
            for j in (i + 1)..rcodes.len() {
                assert_ne!(rcodes[i], rcodes[j], "DNS rcodes must be distinct");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Record types
    // -----------------------------------------------------------------------

    #[test]
    fn test_record_types() {
        assert_eq!(NS_T_A, 1);
        assert_eq!(NS_T_NS, 2);
        assert_eq!(NS_T_CNAME, 5);
        assert_eq!(NS_T_SOA, 6);
        assert_eq!(NS_T_PTR, 12);
        assert_eq!(NS_T_MX, 15);
        assert_eq!(NS_T_TXT, 16);
        assert_eq!(NS_T_AAAA, 28);
        assert_eq!(NS_T_SRV, 33);
    }

    #[test]
    fn test_record_types_distinct() {
        let types = [
            NS_T_A,
            NS_T_NS,
            NS_T_CNAME,
            NS_T_SOA,
            NS_T_WKS,
            NS_T_PTR,
            NS_T_HINFO,
            NS_T_MX,
            NS_T_TXT,
            NS_T_RP,
            NS_T_AFSDB,
            NS_T_AAAA,
            NS_T_LOC,
            NS_T_SRV,
            NS_T_NAPTR,
            NS_T_DNAME,
            NS_T_OPT,
            NS_T_DS,
            NS_T_RRSIG,
            NS_T_NSEC,
            NS_T_DNSKEY,
            NS_T_TLSA,
            NS_T_SVCB,
            NS_T_HTTPS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j], "DNS record types must be distinct");
            }
        }
    }

    #[test]
    fn test_legacy_type_aliases() {
        assert_eq!(T_A, NS_T_A);
        assert_eq!(T_NS, NS_T_NS);
        assert_eq!(T_CNAME, NS_T_CNAME);
        assert_eq!(T_SOA, NS_T_SOA);
        assert_eq!(T_PTR, NS_T_PTR);
        assert_eq!(T_MX, NS_T_MX);
        assert_eq!(T_TXT, NS_T_TXT);
        assert_eq!(T_AAAA, NS_T_AAAA);
        assert_eq!(T_SRV, NS_T_SRV);
        assert_eq!(T_ANY, NS_T_ANY);
    }

    // -----------------------------------------------------------------------
    // Classes
    // -----------------------------------------------------------------------

    #[test]
    fn test_classes() {
        assert_eq!(NS_C_IN, 1);
        assert_eq!(NS_C_CH, 3);
        assert_eq!(NS_C_ANY, 255);
    }

    #[test]
    fn test_classes_distinct() {
        let classes = [NS_C_IN, NS_C_CS, NS_C_CH, NS_C_HS, NS_C_ANY];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_legacy_class_aliases() {
        assert_eq!(C_IN, NS_C_IN);
        assert_eq!(C_ANY, NS_C_ANY);
    }

    // -----------------------------------------------------------------------
    // Utility functions
    // -----------------------------------------------------------------------

    #[test]
    fn test_ns_get16() {
        let buf = [0x01, 0x00]; // 256 in big-endian
        let val = unsafe { ns_get16(buf.as_ptr()) };
        assert_eq!(val, 256);
    }

    #[test]
    fn test_ns_get16_zero() {
        let buf = [0x00, 0x00];
        assert_eq!(unsafe { ns_get16(buf.as_ptr()) }, 0);
    }

    #[test]
    fn test_ns_get16_max() {
        let buf = [0xFF, 0xFF];
        assert_eq!(unsafe { ns_get16(buf.as_ptr()) }, 0xFFFF);
    }

    #[test]
    fn test_ns_get32() {
        let buf = [0x00, 0x00, 0x01, 0x00]; // 256 in big-endian
        let val = unsafe { ns_get32(buf.as_ptr()) };
        assert_eq!(val, 256);
    }

    #[test]
    fn test_ns_get32_large() {
        let buf = [0x7F, 0x00, 0x00, 0x01]; // 127.0.0.1
        let val = unsafe { ns_get32(buf.as_ptr()) };
        assert_eq!(val, 0x7F000001);
    }

    #[test]
    fn test_ns_put16() {
        let mut buf = [0u8; 2];
        unsafe { ns_put16(0x1234, buf.as_mut_ptr()) };
        assert_eq!(buf, [0x12, 0x34]);
    }

    #[test]
    fn test_ns_put32() {
        let mut buf = [0u8; 4];
        unsafe { ns_put32(0xDEADBEEF, buf.as_mut_ptr()) };
        assert_eq!(buf, [0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_get_put_roundtrip_16() {
        let mut buf = [0u8; 2];
        let original: u16 = 12345;
        unsafe {
            ns_put16(original, buf.as_mut_ptr());
            let recovered = ns_get16(buf.as_ptr());
            assert_eq!(recovered, original);
        }
    }

    #[test]
    fn test_get_put_roundtrip_32() {
        let mut buf = [0u8; 4];
        let original: u32 = 0xCAFEBABE;
        unsafe {
            ns_put32(original, buf.as_mut_ptr());
            let recovered = ns_get32(buf.as_ptr());
            assert_eq!(recovered, original);
        }
    }

    // -----------------------------------------------------------------------
    // Cross-module consistency
    // -----------------------------------------------------------------------

    #[test]
    fn test_dns_port_matches_netinet() {
        assert_eq!(NS_DEFAULTPORT, crate::netinet::IPPORT_DNS);
    }
}
