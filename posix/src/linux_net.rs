//! `<linux/net.h>` — Linux networking constants.
//!
//! Provides the `SYS_SOCKET*` multiplexed syscall constants and
//! socket type definitions used by the kernel.

// ---------------------------------------------------------------------------
// Multiplexed socket syscall numbers (for socketcall())
// ---------------------------------------------------------------------------

/// socket() call.
pub const SYS_SOCKET: i32 = 1;
/// bind() call.
pub const SYS_BIND: i32 = 2;
/// connect() call.
pub const SYS_CONNECT: i32 = 3;
/// listen() call.
pub const SYS_LISTEN: i32 = 4;
/// accept() call.
pub const SYS_ACCEPT: i32 = 5;
/// getsockname() call.
pub const SYS_GETSOCKNAME: i32 = 6;
/// getpeername() call.
pub const SYS_GETPEERNAME: i32 = 7;
/// socketpair() call.
pub const SYS_SOCKETPAIR: i32 = 8;
/// send() call.
pub const SYS_SEND: i32 = 9;
/// recv() call.
pub const SYS_RECV: i32 = 10;
/// sendto() call.
pub const SYS_SENDTO: i32 = 11;
/// recvfrom() call.
pub const SYS_RECVFROM: i32 = 12;
/// shutdown() call.
pub const SYS_SHUTDOWN: i32 = 13;
/// setsockopt() call.
pub const SYS_SETSOCKOPT: i32 = 14;
/// getsockopt() call.
pub const SYS_GETSOCKOPT: i32 = 15;
/// sendmsg() call.
pub const SYS_SENDMSG: i32 = 16;
/// recvmsg() call.
pub const SYS_RECVMSG: i32 = 17;
/// accept4() call.
pub const SYS_ACCEPT4: i32 = 18;
/// recvmmsg() call.
pub const SYS_RECVMMSG: i32 = 19;
/// sendmmsg() call.
pub const SYS_SENDMMSG: i32 = 20;

// ---------------------------------------------------------------------------
// Socket state flags (internal kernel state)
// ---------------------------------------------------------------------------

/// Socket is free.
pub const SS_FREE: i32 = 0;
/// Socket not yet connected.
pub const SS_UNCONNECTED: i32 = 1;
/// In process of connecting.
pub const SS_CONNECTING: i32 = 2;
/// Connected to socket.
pub const SS_CONNECTED: i32 = 3;
/// In process of disconnecting.
pub const SS_DISCONNECTING: i32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socketcall_numbers_sequential() {
        assert_eq!(SYS_SOCKET, 1);
        assert_eq!(SYS_BIND, 2);
        assert_eq!(SYS_CONNECT, 3);
        assert_eq!(SYS_SENDMMSG, 20);
    }

    #[test]
    fn test_socketcall_numbers_distinct() {
        let nums = [
            SYS_SOCKET,
            SYS_BIND,
            SYS_CONNECT,
            SYS_LISTEN,
            SYS_ACCEPT,
            SYS_GETSOCKNAME,
            SYS_GETPEERNAME,
            SYS_SOCKETPAIR,
            SYS_SEND,
            SYS_RECV,
            SYS_SENDTO,
            SYS_RECVFROM,
            SYS_SHUTDOWN,
            SYS_SETSOCKOPT,
            SYS_GETSOCKOPT,
            SYS_SENDMSG,
            SYS_RECVMSG,
            SYS_ACCEPT4,
            SYS_RECVMMSG,
            SYS_SENDMMSG,
        ];
        for i in 0..nums.len() {
            for j in (i + 1)..nums.len() {
                assert_ne!(nums[i], nums[j]);
            }
        }
    }

    #[test]
    fn test_socket_states() {
        assert_eq!(SS_FREE, 0);
        assert_eq!(SS_CONNECTED, 3);
        let states = [
            SS_FREE,
            SS_UNCONNECTED,
            SS_CONNECTING,
            SS_CONNECTED,
            SS_DISCONNECTING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
