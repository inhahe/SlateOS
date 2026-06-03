//! `<linux/net.h>` — socket-layer constants shared across all address families.
//!
//! These constants underlie every `socket(2)` call regardless of the
//! address family. The `SOCK_*` types, the `SOL_SOCKET` level, and the
//! per-syscall numbers come from here. Higher-level files (`linux_ip`,
//! `linux_ipv6`, `linux_unix`, etc.) layer family-specific options on
//! top.

// ---------------------------------------------------------------------------
// Socket types (passed as the second argument to `socket(2)`)
// ---------------------------------------------------------------------------

pub const SOCK_STREAM: u32 = 1;
pub const SOCK_DGRAM: u32 = 2;
pub const SOCK_RAW: u32 = 3;
pub const SOCK_RDM: u32 = 4;
pub const SOCK_SEQPACKET: u32 = 5;
pub const SOCK_DCCP: u32 = 6;
pub const SOCK_PACKET: u32 = 10;

/// Mask covering the type bits in the socket-type argument.
pub const SOCK_TYPE_MASK: u32 = 0xF;

/// Flag: open the new socket with FD_CLOEXEC.
pub const SOCK_CLOEXEC: u32 = 0o2_000_000;
/// Flag: open the new socket in non-blocking mode.
pub const SOCK_NONBLOCK: u32 = 0o4000;

// ---------------------------------------------------------------------------
// Generic socket levels
// ---------------------------------------------------------------------------

pub const SOL_SOCKET: u32 = 1;
pub const SOL_IP: u32 = 0;
pub const SOL_TCP: u32 = 6;
pub const SOL_UDP: u32 = 17;
pub const SOL_IPV6: u32 = 41;

// ---------------------------------------------------------------------------
// `shutdown(2)` directions
// ---------------------------------------------------------------------------

pub const SHUT_RD: u32 = 0;
pub const SHUT_WR: u32 = 1;
pub const SHUT_RDWR: u32 = 2;

// ---------------------------------------------------------------------------
// `sendto(2)` / `recvfrom(2)` flags
// ---------------------------------------------------------------------------

pub const MSG_OOB: u32 = 0x0001;
pub const MSG_PEEK: u32 = 0x0002;
pub const MSG_DONTROUTE: u32 = 0x0004;
pub const MSG_CTRUNC: u32 = 0x0008;
pub const MSG_PROXY: u32 = 0x0010;
pub const MSG_TRUNC: u32 = 0x0020;
pub const MSG_DONTWAIT: u32 = 0x0040;
pub const MSG_EOR: u32 = 0x0080;
pub const MSG_WAITALL: u32 = 0x0100;
pub const MSG_FIN: u32 = 0x0200;
pub const MSG_SYN: u32 = 0x0400;
pub const MSG_CONFIRM: u32 = 0x0800;
pub const MSG_RST: u32 = 0x1000;
pub const MSG_ERRQUEUE: u32 = 0x2000;
pub const MSG_NOSIGNAL: u32 = 0x4000;
pub const MSG_MORE: u32 = 0x8000;
pub const MSG_WAITFORONE: u32 = 0x1_0000;
pub const MSG_BATCH: u32 = 0x4_0000;
pub const MSG_ZEROCOPY: u32 = 0x400_0000;
pub const MSG_FASTOPEN: u32 = 0x2000_0000;
pub const MSG_CMSG_CLOEXEC: u32 = 0x4000_0000;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_SOCKET: u32 = 41;
pub const NR_CONNECT: u32 = 42;
pub const NR_ACCEPT: u32 = 43;
pub const NR_SENDTO: u32 = 44;
pub const NR_RECVFROM: u32 = 45;
pub const NR_SENDMSG: u32 = 46;
pub const NR_RECVMSG: u32 = 47;
pub const NR_SHUTDOWN: u32 = 48;
pub const NR_BIND: u32 = 49;
pub const NR_LISTEN: u32 = 50;
pub const NR_SETSOCKOPT: u32 = 54;
pub const NR_GETSOCKOPT: u32 = 55;
pub const NR_ACCEPT4: u32 = 288;
pub const NR_RECVMMSG: u32 = 299;
pub const NR_SENDMMSG: u32 = 307;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sock_types_dense_1_to_5() {
        let s = [
            SOCK_STREAM,
            SOCK_DGRAM,
            SOCK_RAW,
            SOCK_RDM,
            SOCK_SEQPACKET,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
        // SOCK_TYPE_MASK covers all 16 possible types.
        assert_eq!(SOCK_TYPE_MASK, 0xF);
        assert!(SOCK_PACKET <= SOCK_TYPE_MASK);
    }

    #[test]
    fn test_sock_flags_outside_type_field() {
        // CLOEXEC and NONBLOCK must not collide with the type mask.
        assert_eq!(SOCK_CLOEXEC & SOCK_TYPE_MASK, 0);
        assert_eq!(SOCK_NONBLOCK & SOCK_TYPE_MASK, 0);
        assert_ne!(SOCK_CLOEXEC, SOCK_NONBLOCK);
    }

    #[test]
    fn test_sol_levels() {
        // SOL_IP=0 (also IPPROTO_IP), SOL_SOCKET=1, SOL_TCP=6, etc.
        assert_eq!(SOL_IP, 0);
        assert_eq!(SOL_SOCKET, 1);
        assert_eq!(SOL_TCP, 6);
        assert_eq!(SOL_UDP, 17);
        assert_eq!(SOL_IPV6, 41);
    }

    #[test]
    fn test_shutdown_dense_0_to_2() {
        assert_eq!(SHUT_RD, 0);
        assert_eq!(SHUT_WR, 1);
        assert_eq!(SHUT_RDWR, 2);
    }

    #[test]
    fn test_msg_flags_single_bit_and_distinct() {
        let m = [
            MSG_OOB,
            MSG_PEEK,
            MSG_DONTROUTE,
            MSG_CTRUNC,
            MSG_PROXY,
            MSG_TRUNC,
            MSG_DONTWAIT,
            MSG_EOR,
            MSG_WAITALL,
            MSG_FIN,
            MSG_SYN,
            MSG_CONFIRM,
            MSG_RST,
            MSG_ERRQUEUE,
            MSG_NOSIGNAL,
            MSG_MORE,
            MSG_WAITFORONE,
            MSG_BATCH,
            MSG_ZEROCOPY,
            MSG_FASTOPEN,
            MSG_CMSG_CLOEXEC,
        ];
        for v in m {
            assert!(v.is_power_of_two());
        }
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
    }

    #[test]
    fn test_socket_syscall_block_41_to_55() {
        // Dense 41..55 except 51..53 (which are other syscalls).
        assert_eq!(NR_SOCKET, 41);
        assert_eq!(NR_LISTEN, 50);
        assert_eq!(NR_SETSOCKOPT, 54);
        assert_eq!(NR_GETSOCKOPT, 55);
        for w in [
            NR_SOCKET, NR_CONNECT, NR_ACCEPT, NR_SENDTO, NR_RECVFROM, NR_SENDMSG, NR_RECVMSG,
            NR_SHUTDOWN, NR_BIND, NR_LISTEN,
        ]
        .windows(2)
        {
            assert_eq!(w[1], w[0] + 1);
        }
    }
}
