//! `<ifaddrs.h>` — network interface address enumeration.
//!
//! Re-exports `getifaddrs()`, `freeifaddrs()`, and the `Ifaddrs`
//! struct from the `socket` module.

pub use crate::socket::Ifaddrs;
pub use crate::socket::freeifaddrs;
pub use crate::socket::getifaddrs;

// Re-export address family constants commonly used with ifaddrs.
pub use crate::net_if_packet::AF_PACKET;
pub use crate::socket::AF_INET;
pub use crate::socket::AF_INET6;
pub use crate::socket::AF_UNIX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifaddrs_struct_size() {
        assert!(core::mem::size_of::<Ifaddrs>() > 0);
    }

    #[test]
    fn test_af_constants() {
        assert_eq!(AF_INET, 2);
        assert_eq!(AF_INET6, 10);
        assert_eq!(AF_UNIX, 1);
    }

    #[test]
    fn test_getifaddrs_stub() {
        let mut ptr: *mut Ifaddrs = core::ptr::null_mut();
        // getifaddrs is unsafe extern "C"
        let ret = unsafe { getifaddrs(&mut ptr) };
        // Stub returns 0 with a linked list.
        assert_eq!(ret, 0);
        assert!(!ptr.is_null());
        // freeifaddrs is safe
        freeifaddrs(ptr);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(AF_INET, crate::socket::AF_INET);
        assert_eq!(AF_PACKET, crate::net_if_packet::AF_PACKET);
    }
}
