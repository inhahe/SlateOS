//! `<netdb.h>` — network database operations.
//!
//! This module re-exports constants from `<netdb.h>` that are
//! implemented in the `socket` module.  Programs that include
//! `<netdb.h>` for constants like `HOST_NOT_FOUND`, `NI_MAXHOST`,
//! or `AI_PASSIVE` can find them here.
//!
//! The actual function implementations (`getaddrinfo`, `gethostbyname`,
//! `getservbyname`, `getprotobyname`, `herror`, `hstrerror`, etc.)
//! all live in the `socket` module.

// ---------------------------------------------------------------------------
// Re-exports from socket module
// ---------------------------------------------------------------------------

// h_errno error codes
pub use crate::socket::HOST_NOT_FOUND;
pub use crate::socket::TRY_AGAIN;
pub use crate::socket::NO_RECOVERY;
pub use crate::socket::NO_DATA;

/// Alias for `NO_DATA` (POSIX).
pub const NO_ADDRESS: i32 = NO_DATA;

// AI_* flags for getaddrinfo
pub use crate::socket::AI_PASSIVE;
pub use crate::socket::AI_NUMERICHOST;

// NI_* constants for getnameinfo
/// Maximum hostname length.
pub const NI_MAXHOST: usize = 1025;
/// Maximum service name length.
pub const NI_MAXSERV: usize = 32;

pub use crate::socket::NI_NUMERICHOST;
pub use crate::socket::NI_NUMERICSERV;

// EAI_* error codes for getaddrinfo
pub use crate::socket::EAI_AGAIN;
pub use crate::socket::EAI_BADFLAGS;
pub use crate::socket::EAI_FAIL;
pub use crate::socket::EAI_FAMILY;
pub use crate::socket::EAI_MEMORY;
pub use crate::socket::EAI_NONAME;
pub use crate::socket::EAI_SERVICE;
pub use crate::socket::EAI_SOCKTYPE;
pub use crate::socket::EAI_SYSTEM;

// Struct re-exports
pub use crate::socket::Addrinfo;
pub use crate::socket::Servent;
pub use crate::socket::Protoent;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // h_errno constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_h_errno_constants() {
        assert_eq!(HOST_NOT_FOUND, 1);
        assert_eq!(TRY_AGAIN, 2);
        assert_eq!(NO_RECOVERY, 3);
        assert_eq!(NO_DATA, 4);
        assert_eq!(NO_ADDRESS, NO_DATA);
    }

    #[test]
    fn test_h_errno_constants_distinct() {
        let vals = [HOST_NOT_FOUND, TRY_AGAIN, NO_RECOVERY, NO_DATA];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_h_errno_constants_positive() {
        assert!(HOST_NOT_FOUND > 0);
        assert!(TRY_AGAIN > 0);
        assert!(NO_RECOVERY > 0);
        assert!(NO_DATA > 0);
    }

    // -----------------------------------------------------------------------
    // AI_* flags accessible through this module
    // -----------------------------------------------------------------------

    #[test]
    fn test_ai_flags_accessible() {
        assert_ne!(AI_PASSIVE, 0);
        assert_ne!(AI_NUMERICHOST, 0);
        assert_ne!(AI_PASSIVE, AI_NUMERICHOST);
    }

    // -----------------------------------------------------------------------
    // NI_* constants accessible through this module
    // -----------------------------------------------------------------------

    #[test]
    fn test_ni_max_accessible() {
        assert!(NI_MAXHOST > 0);
        assert!(NI_MAXSERV > 0);
    }

    // -----------------------------------------------------------------------
    // EAI_* error codes accessible through this module
    // -----------------------------------------------------------------------

    #[test]
    fn test_eai_codes_accessible() {
        // All EAI codes should be non-zero (they indicate errors).
        assert_ne!(EAI_AGAIN, 0);
        assert_ne!(EAI_BADFLAGS, 0);
        assert_ne!(EAI_FAIL, 0);
        assert_ne!(EAI_FAMILY, 0);
        assert_ne!(EAI_MEMORY, 0);
        assert_ne!(EAI_NONAME, 0);
        assert_ne!(EAI_SERVICE, 0);
        assert_ne!(EAI_SOCKTYPE, 0);
        assert_ne!(EAI_SYSTEM, 0);
    }

    #[test]
    fn test_eai_codes_distinct() {
        let codes = [
            EAI_AGAIN, EAI_BADFLAGS, EAI_FAIL, EAI_FAMILY,
            EAI_MEMORY, EAI_NONAME, EAI_SERVICE, EAI_SOCKTYPE,
            EAI_SYSTEM,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(
                    codes[i], codes[j],
                    "EAI codes must be distinct"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Service lookup stubs (delegated to socket module)
    // -----------------------------------------------------------------------

    #[test]
    fn test_getservbyname_returns_null() {
        let result = unsafe {
            crate::socket::getservbyname(
                b"http\0".as_ptr(),
                b"tcp\0".as_ptr(),
            )
        };
        assert!(result.is_null());
    }

    #[test]
    fn test_getprotobynumber_returns_null() {
        let result = crate::socket::getprotobynumber(6); // TCP
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // herror / hstrerror (in socket module)
    // -----------------------------------------------------------------------

    #[test]
    fn test_herror_no_crash() {
        crate::socket::herror(core::ptr::null());
    }

    #[test]
    fn test_hstrerror_returns_non_null() {
        let s = crate::socket::hstrerror(HOST_NOT_FOUND);
        assert!(!s.is_null());
    }
}
