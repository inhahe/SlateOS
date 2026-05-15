//! `<sys/socket.h>` — socket definitions.
//!
//! Re-exports socket types, address families, protocol families,
//! socket options, and socket functions from the `socket` module.

// ---------------------------------------------------------------------------
// Address families
// ---------------------------------------------------------------------------

pub use crate::socket::AF_UNSPEC;
pub use crate::socket::AF_UNIX;
pub use crate::socket::AF_LOCAL;
pub use crate::socket::AF_INET;
pub use crate::socket::AF_INET6;

// ---------------------------------------------------------------------------
// Protocol families (aliases for AF_*)
// ---------------------------------------------------------------------------

pub use crate::socket::PF_UNIX;
pub use crate::socket::PF_INET;
pub use crate::socket::PF_INET6;

// ---------------------------------------------------------------------------
// Socket types
// ---------------------------------------------------------------------------

pub use crate::socket::SOCK_STREAM;
pub use crate::socket::SOCK_DGRAM;
pub use crate::socket::SOCK_RAW;
pub use crate::socket::SOCK_SEQPACKET;
pub use crate::socket::SOCK_NONBLOCK;
pub use crate::socket::SOCK_CLOEXEC;

// ---------------------------------------------------------------------------
// Shutdown modes
// ---------------------------------------------------------------------------

pub use crate::socket::SHUT_RD;
pub use crate::socket::SHUT_WR;
pub use crate::socket::SHUT_RDWR;

// ---------------------------------------------------------------------------
// Socket-level options
// ---------------------------------------------------------------------------

pub use crate::socket::SOL_SOCKET;
pub use crate::socket::SO_REUSEADDR;
pub use crate::socket::SO_KEEPALIVE;
pub use crate::socket::SO_TYPE;
pub use crate::socket::SO_ERROR;
pub use crate::socket::SO_RCVBUF;
pub use crate::socket::SO_SNDBUF;
pub use crate::socket::SO_BROADCAST;
pub use crate::socket::SO_LINGER;
pub use crate::socket::SO_REUSEPORT;
pub use crate::socket::SO_RCVTIMEO;
pub use crate::socket::SO_SNDTIMEO;
pub use crate::socket::SO_ACCEPTCONN;
pub use crate::socket::SO_DOMAIN;
pub use crate::socket::SO_PROTOCOL;
pub use crate::socket::SO_RCVLOWAT;
pub use crate::socket::SO_SNDLOWAT;

// ---------------------------------------------------------------------------
// Message flags
// ---------------------------------------------------------------------------

pub use crate::socket::MSG_OOB;
pub use crate::socket::MSG_PEEK;
pub use crate::socket::MSG_DONTROUTE;
pub use crate::socket::MSG_TRUNC;
pub use crate::socket::MSG_DONTWAIT;
pub use crate::socket::MSG_EOR;
pub use crate::socket::MSG_WAITALL;
pub use crate::socket::MSG_MORE;
pub use crate::socket::MSG_NOSIGNAL;
pub use crate::socket::MSG_CMSG_CLOEXEC;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

pub use crate::socket::Sockaddr;
pub use crate::socket::SockaddrIn;
pub use crate::socket::SockaddrIn6;
pub use crate::socket::SockaddrUn;
pub use crate::socket::SockaddrStorage;
pub use crate::socket::Iovec;
pub use crate::socket::Msghdr;
pub use crate::socket::Cmsghdr;
pub use crate::socket::SocklenT;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

pub use crate::socket::socket;
pub use crate::socket::listen;
pub use crate::socket::shutdown;
pub use crate::socket::setsockopt;
pub use crate::socket::freeaddrinfo;
pub use crate::socket::getnameinfo;
pub use crate::socket::socketpair;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_values() {
        assert_eq!(AF_UNSPEC, 0);
        assert_eq!(AF_UNIX, 1);
        assert_eq!(AF_LOCAL, AF_UNIX);
        assert_eq!(AF_INET, 2);
        assert_eq!(AF_INET6, 10);
    }

    #[test]
    fn test_sock_types() {
        assert_eq!(SOCK_STREAM, 1);
        assert_eq!(SOCK_DGRAM, 2);
        assert_eq!(SOCK_RAW, 3);
    }

    #[test]
    fn test_shut_values() {
        assert_eq!(SHUT_RD, 0);
        assert_eq!(SHUT_WR, 1);
        assert_eq!(SHUT_RDWR, 2);
    }

    #[test]
    fn test_sol_socket() {
        assert_eq!(SOL_SOCKET, 1);
    }

    #[test]
    fn test_msg_flags_distinct() {
        let flags = [
            MSG_OOB, MSG_PEEK, MSG_DONTROUTE, MSG_TRUNC,
            MSG_DONTWAIT, MSG_EOR, MSG_WAITALL,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j], "MSG flags must be distinct");
            }
        }
    }

    #[test]
    fn test_sockaddr_size() {
        assert_eq!(core::mem::size_of::<Sockaddr>(), 16);
    }

    #[test]
    fn test_sockaddr_in_size() {
        assert_eq!(core::mem::size_of::<SockaddrIn>(), 16);
    }

    #[test]
    fn test_sockaddr_storage_size() {
        assert!(core::mem::size_of::<SockaddrStorage>() >= 128);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(AF_INET, crate::socket::AF_INET);
        assert_eq!(SOCK_STREAM, crate::socket::SOCK_STREAM);
        assert_eq!(SOL_SOCKET, crate::socket::SOL_SOCKET);
        assert_eq!(SHUT_RDWR, crate::socket::SHUT_RDWR);
    }
}
