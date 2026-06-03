//! `<arpa/inet.h>` — Internet address conversion functions.
//!
//! Re-exports network byte-order and address conversion functions
//! from the `socket` module.

// ---------------------------------------------------------------------------
// Byte-order conversion
// ---------------------------------------------------------------------------

pub use crate::socket::htonl;
pub use crate::socket::htons;
pub use crate::socket::ntohl;
pub use crate::socket::ntohs;

// ---------------------------------------------------------------------------
// Address conversion
// ---------------------------------------------------------------------------

pub use crate::socket::inet_addr;
pub use crate::socket::inet_aton;
pub use crate::socket::inet_ntoa;
pub use crate::socket::inet_ntop;
pub use crate::socket::inet_pton;

// ---------------------------------------------------------------------------
// Types and constants
// ---------------------------------------------------------------------------

pub use crate::socket::AF_INET;
pub use crate::socket::AF_INET6;
pub use crate::socket::INADDR_ANY;
pub use crate::socket::INADDR_BROADCAST;
pub use crate::socket::INADDR_LOOPBACK;
pub use crate::socket::INADDR_NONE;
pub use crate::socket::INET_ADDRSTRLEN;
pub use crate::socket::INET6_ADDRSTRLEN;
pub use crate::socket::In6Addr;
pub use crate::socket::InAddr;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_htons_ntohs_roundtrip() {
        let val: u16 = 0x1234;
        assert_eq!(ntohs(htons(val)), val);
    }

    #[test]
    fn test_htonl_ntohl_roundtrip() {
        let val: u32 = 0x12345678;
        assert_eq!(ntohl(htonl(val)), val);
    }

    #[test]
    fn test_inet_addr_loopback() {
        let addr = unsafe { inet_addr(b"127.0.0.1\0".as_ptr()) };
        assert_ne!(addr, INADDR_NONE);
    }

    #[test]
    fn test_inet_addr_invalid() {
        let addr = unsafe { inet_addr(b"not.an.ip\0".as_ptr()) };
        assert_eq!(addr, INADDR_NONE);
    }

    #[test]
    fn test_inet_pton_ipv4() {
        let mut buf = [0u8; 4];
        let ret = unsafe { inet_pton(AF_INET, b"192.168.1.1\0".as_ptr(), buf.as_mut_ptr()) };
        assert_eq!(ret, 1);
        assert_eq!(buf, [192, 168, 1, 1]);
    }

    #[test]
    fn test_inet_ntop_ipv4() {
        let addr: [u8; 4] = [10, 0, 0, 1];
        let mut buf = [0u8; INET_ADDRSTRLEN as usize];
        let p = inet_ntop(
            AF_INET,
            addr.as_ptr(),
            buf.as_mut_ptr(),
            INET_ADDRSTRLEN as u32,
        );
        assert!(!p.is_null());
        // Read back the string.
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        let s = core::str::from_utf8(&buf[..len]).unwrap();
        assert_eq!(s, "10.0.0.1");
    }

    #[test]
    fn test_inet_aton_valid() {
        let mut addr: u32 = 0;
        let ret = unsafe { inet_aton(b"1.2.3.4\0".as_ptr(), &mut addr as *mut u32) };
        assert_eq!(ret, 1);
    }

    #[test]
    fn test_address_constants() {
        assert_eq!(INADDR_ANY, 0);
        assert_ne!(INADDR_LOOPBACK, 0);
        assert_ne!(INADDR_BROADCAST, 0);
    }

    #[test]
    fn test_addrstrlen_values() {
        assert_eq!(INET_ADDRSTRLEN, 16);
        assert_eq!(INET6_ADDRSTRLEN, 46);
    }

    #[test]
    fn test_cross_module_htons() {
        let val: u16 = 80;
        assert_eq!(htons(val), crate::socket::htons(val));
    }

    #[test]
    fn test_cross_module_inet_addr() {
        let a = unsafe { inet_addr(b"10.0.0.1\0".as_ptr()) };
        let b = unsafe { crate::socket::inet_addr(b"10.0.0.1\0".as_ptr()) };
        assert_eq!(a, b);
    }
}
