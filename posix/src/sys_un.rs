//! `<sys/un.h>` — Unix domain socket address.
//!
//! Re-exports `SockaddrUn` and related constants from the `socket`
//! module.

pub use crate::socket::SockaddrUn;
pub use crate::socket::AF_UNIX;
pub use crate::socket::AF_LOCAL;

/// Maximum path length in a Unix domain socket address.
///
/// This matches the size of `SockaddrUn::sun_path`.
pub const UNIX_PATH_MAX: usize = 108;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sockaddr_un_size() {
        assert_eq!(core::mem::size_of::<SockaddrUn>(), 110);
    }

    #[test]
    fn test_unix_path_max() {
        assert_eq!(UNIX_PATH_MAX, 108);
    }

    #[test]
    fn test_af_unix_value() {
        assert_eq!(AF_UNIX, 1);
        assert_eq!(AF_LOCAL, AF_UNIX);
    }

    #[test]
    fn test_sockaddr_un_init() {
        let addr = SockaddrUn {
            sun_family: AF_UNIX as u16,
            sun_path: [0u8; 108],
        };
        assert_eq!(addr.sun_family, 1);
    }

    #[test]
    fn test_sockaddr_un_path() {
        let mut addr = SockaddrUn {
            sun_family: AF_UNIX as u16,
            sun_path: [0u8; 108],
        };
        let path = b"/tmp/test.sock";
        addr.sun_path[..path.len()].copy_from_slice(path);
        assert_eq!(&addr.sun_path[..14], b"/tmp/test.sock");
        assert_eq!(addr.sun_path[14], 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(AF_UNIX, crate::socket::AF_UNIX);
        assert_eq!(AF_LOCAL, crate::socket::AF_LOCAL);
    }
}
