//! `<linux/sock_cgroup.h>` — Socket cgroup control constants.
//!
//! The socket cgroup subsystem allows binding sockets to specific
//! cgroups for network traffic classification, filtering, and
//! accounting. eBPF programs attached to cgroup hooks can control
//! socket operations: allowing/denying connections, modifying socket
//! options, redirecting traffic, and implementing per-cgroup network
//! policies. This enables fine-grained container network control
//! without traditional iptables rules.

// ---------------------------------------------------------------------------
// cgroup/BPF attach types for sockets
// ---------------------------------------------------------------------------

/// Ingress filter (packets arriving at socket).
pub const BPF_CGROUP_INET_INGRESS: u32 = 0;
/// Egress filter (packets leaving socket).
pub const BPF_CGROUP_INET_EGRESS: u32 = 1;
/// Socket create hook (allow/deny socket creation).
pub const BPF_CGROUP_INET_SOCK_CREATE: u32 = 2;
/// Socket release hook (cleanup on close).
pub const BPF_CGROUP_INET_SOCK_RELEASE: u32 = 3;
/// Bind hook (IPv4, allow/deny/modify bind address).
pub const BPF_CGROUP_INET4_BIND: u32 = 4;
/// Bind hook (IPv6).
pub const BPF_CGROUP_INET6_BIND: u32 = 5;
/// Connect hook (IPv4, allow/deny/modify connect address).
pub const BPF_CGROUP_INET4_CONNECT: u32 = 6;
/// Connect hook (IPv6).
pub const BPF_CGROUP_INET6_CONNECT: u32 = 7;
/// Post-bind hook (IPv4, after address assigned).
pub const BPF_CGROUP_INET4_POST_BIND: u32 = 8;
/// Post-bind hook (IPv6).
pub const BPF_CGROUP_INET6_POST_BIND: u32 = 9;
/// Sendmsg hook (UDP IPv4, modify destination).
pub const BPF_CGROUP_UDP4_SENDMSG: u32 = 10;
/// Sendmsg hook (UDP IPv6).
pub const BPF_CGROUP_UDP6_SENDMSG: u32 = 11;
/// Getsockopt hook (allow/deny/modify).
pub const BPF_CGROUP_GETSOCKOPT: u32 = 12;
/// Setsockopt hook (allow/deny/modify).
pub const BPF_CGROUP_SETSOCKOPT: u32 = 13;

// ---------------------------------------------------------------------------
// BPF socket return values
// ---------------------------------------------------------------------------

/// Allow the operation.
pub const BPF_SOCK_OPS_OK: u32 = 0;
/// Deny the operation.
pub const BPF_SOCK_OPS_DENY: u32 = 1;

// ---------------------------------------------------------------------------
// Socket cgroup flags
// ---------------------------------------------------------------------------

/// Cgroup socket accounting is enabled.
pub const SOCK_CGROUP_ACCOUNTING: u32 = 0x01;
/// Cgroup classid is set on this socket.
pub const SOCK_CGROUP_CLASSID: u32 = 0x02;
/// Cgroup net_prio is set on this socket.
pub const SOCK_CGROUP_NETPRIO: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attach_types_distinct() {
        let types = [
            BPF_CGROUP_INET_INGRESS, BPF_CGROUP_INET_EGRESS,
            BPF_CGROUP_INET_SOCK_CREATE, BPF_CGROUP_INET_SOCK_RELEASE,
            BPF_CGROUP_INET4_BIND, BPF_CGROUP_INET6_BIND,
            BPF_CGROUP_INET4_CONNECT, BPF_CGROUP_INET6_CONNECT,
            BPF_CGROUP_INET4_POST_BIND, BPF_CGROUP_INET6_POST_BIND,
            BPF_CGROUP_UDP4_SENDMSG, BPF_CGROUP_UDP6_SENDMSG,
            BPF_CGROUP_GETSOCKOPT, BPF_CGROUP_SETSOCKOPT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_return_values_distinct() {
        assert_ne!(BPF_SOCK_OPS_OK, BPF_SOCK_OPS_DENY);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            SOCK_CGROUP_ACCOUNTING, SOCK_CGROUP_CLASSID,
            SOCK_CGROUP_NETPRIO,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
