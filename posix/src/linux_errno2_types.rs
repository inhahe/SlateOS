//! `<errno.h>` — Extended POSIX error number constants (35-133).
//!
//! These are the extended error codes beyond the classic POSIX set,
//! covering networking errors, filesystem-specific errors, and
//! Linux-specific conditions.

// ---------------------------------------------------------------------------
// Extended errno values (35-80)
// ---------------------------------------------------------------------------

/// Resource deadlock would occur.
pub const EDEADLK: u32 = 35;
/// File name too long.
pub const ENAMETOOLONG: u32 = 36;
/// No record locks available.
pub const ENOLCK: u32 = 37;
/// Function not implemented.
pub const ENOSYS: u32 = 38;
/// Directory not empty.
pub const ENOTEMPTY: u32 = 39;
/// Too many symbolic links encountered.
pub const ELOOP: u32 = 40;
/// No message of desired type.
pub const ENOMSG: u32 = 42;
/// Identifier removed.
pub const EIDRM: u32 = 43;
/// Operation not supported.
pub const ENOTSUP: u32 = 95;

// ---------------------------------------------------------------------------
// Network error codes
// ---------------------------------------------------------------------------

/// Network is down.
pub const ENETDOWN: u32 = 100;
/// Network is unreachable.
pub const ENETUNREACH: u32 = 101;
/// Network dropped connection on reset.
pub const ENETRESET: u32 = 102;
/// Software caused connection abort.
pub const ECONNABORTED: u32 = 103;
/// Connection reset by peer.
pub const ECONNRESET: u32 = 104;
/// No buffer space available.
pub const ENOBUFS: u32 = 105;
/// Transport endpoint already connected.
pub const EISCONN: u32 = 106;
/// Transport endpoint not connected.
pub const ENOTCONN: u32 = 107;
/// Connection timed out.
pub const ETIMEDOUT: u32 = 110;
/// Connection refused.
pub const ECONNREFUSED: u32 = 111;
/// Host is down.
pub const EHOSTDOWN: u32 = 112;
/// No route to host.
pub const EHOSTUNREACH: u32 = 113;
/// Operation already in progress.
pub const EALREADY: u32 = 114;
/// Operation now in progress.
pub const EINPROGRESS: u32 = 115;

// ---------------------------------------------------------------------------
// Socket/address errors
// ---------------------------------------------------------------------------

/// Socket operation on non-socket.
pub const ENOTSOCK: u32 = 88;
/// Destination address required.
pub const EDESTADDRREQ: u32 = 89;
/// Message too long.
pub const EMSGSIZE: u32 = 90;
/// Protocol not available.
pub const ENOPROTOOPT: u32 = 92;
/// Protocol not supported.
pub const EPROTONOSUPPORT: u32 = 93;
/// Address already in use.
pub const EADDRINUSE: u32 = 98;
/// Cannot assign requested address.
pub const EADDRNOTAVAIL: u32 = 99;
/// Address family not supported.
pub const EAFNOSUPPORT: u32 = 97;

// ---------------------------------------------------------------------------
// Linux-specific
// ---------------------------------------------------------------------------

/// Stale file handle (NFS).
pub const ESTALE: u32 = 116;
/// Quota exceeded.
pub const EDQUOT: u32 = 122;
/// Operation canceled.
pub const ECANCELED: u32 = 125;
/// Owner died (robust mutex).
pub const EOWNERDEAD: u32 = 130;
/// State not recoverable (robust mutex).
pub const ENOTRECOVERABLE: u32 = 131;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extended_errnos_distinct() {
        let errs = [
            EDEADLK,
            ENAMETOOLONG,
            ENOLCK,
            ENOSYS,
            ENOTEMPTY,
            ELOOP,
            ENOMSG,
            EIDRM,
            ENOTSUP,
        ];
        for i in 0..errs.len() {
            for j in (i + 1)..errs.len() {
                assert_ne!(errs[i], errs[j]);
            }
        }
    }

    #[test]
    fn test_network_errnos_distinct() {
        let errs = [
            ENETDOWN,
            ENETUNREACH,
            ENETRESET,
            ECONNABORTED,
            ECONNRESET,
            ENOBUFS,
            EISCONN,
            ENOTCONN,
            ETIMEDOUT,
            ECONNREFUSED,
            EHOSTDOWN,
            EHOSTUNREACH,
            EALREADY,
            EINPROGRESS,
        ];
        for i in 0..errs.len() {
            for j in (i + 1)..errs.len() {
                assert_ne!(errs[i], errs[j]);
            }
        }
    }

    #[test]
    fn test_socket_errnos_distinct() {
        let errs = [
            ENOTSOCK,
            EDESTADDRREQ,
            EMSGSIZE,
            ENOPROTOOPT,
            EPROTONOSUPPORT,
            EADDRINUSE,
            EADDRNOTAVAIL,
            EAFNOSUPPORT,
        ];
        for i in 0..errs.len() {
            for j in (i + 1)..errs.len() {
                assert_ne!(errs[i], errs[j]);
            }
        }
    }

    #[test]
    fn test_common_network_values() {
        assert_eq!(ECONNREFUSED, 111);
        assert_eq!(ETIMEDOUT, 110);
        assert_eq!(ECONNRESET, 104);
    }

    #[test]
    fn test_enosys() {
        assert_eq!(ENOSYS, 38);
    }
}
