//! `<linux/bpf.h>` — BPF cgroup attachment constants.
//!
//! Constants for BPF programs attached to cgroups covering
//! attach types, attach flags, and cgroup-specific operations.

// ---------------------------------------------------------------------------
// BPF cgroup attach types (BPF_CGROUP_*)
// ---------------------------------------------------------------------------

/// Ingress.
pub const BPF_CGROUP_INET_INGRESS: u32 = 0;
/// Egress.
pub const BPF_CGROUP_INET_EGRESS: u32 = 1;
/// Socket create.
pub const BPF_CGROUP_INET_SOCK_CREATE: u32 = 2;
/// Socket operations.
pub const BPF_CGROUP_SOCK_OPS: u32 = 3;
/// Device (cgroup device controller).
pub const BPF_CGROUP_DEVICE: u32 = 4;
/// Bind4.
pub const BPF_CGROUP_INET4_BIND: u32 = 5;
/// Bind6.
pub const BPF_CGROUP_INET6_BIND: u32 = 6;
/// Connect4.
pub const BPF_CGROUP_INET4_CONNECT: u32 = 7;
/// Connect6.
pub const BPF_CGROUP_INET6_CONNECT: u32 = 8;
/// Post bind4.
pub const BPF_CGROUP_INET4_POST_BIND: u32 = 9;
/// Post bind6.
pub const BPF_CGROUP_INET6_POST_BIND: u32 = 10;
/// UDP4 sendmsg.
pub const BPF_CGROUP_UDP4_SENDMSG: u32 = 11;
/// UDP6 sendmsg.
pub const BPF_CGROUP_UDP6_SENDMSG: u32 = 12;
/// Sysctl.
pub const BPF_CGROUP_SYSCTL: u32 = 13;
/// UDP4 recvmsg.
pub const BPF_CGROUP_UDP4_RECVMSG: u32 = 14;
/// UDP6 recvmsg.
pub const BPF_CGROUP_UDP6_RECVMSG: u32 = 15;
/// Getsockopt.
pub const BPF_CGROUP_GETSOCKOPT: u32 = 16;
/// Setsockopt.
pub const BPF_CGROUP_SETSOCKOPT: u32 = 17;
/// Socket release.
pub const BPF_CGROUP_INET_SOCK_RELEASE: u32 = 18;

// ---------------------------------------------------------------------------
// BPF cgroup attach flags
// ---------------------------------------------------------------------------

/// Allow multi-attach.
pub const BPF_F_ALLOW_MULTI: u32 = 1 << 1;
/// Replace existing program.
pub const BPF_F_REPLACE: u32 = 1 << 2;

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
            BPF_CGROUP_INET_SOCK_CREATE, BPF_CGROUP_SOCK_OPS,
            BPF_CGROUP_DEVICE, BPF_CGROUP_INET4_BIND,
            BPF_CGROUP_INET6_BIND, BPF_CGROUP_INET4_CONNECT,
            BPF_CGROUP_INET6_CONNECT, BPF_CGROUP_INET4_POST_BIND,
            BPF_CGROUP_INET6_POST_BIND, BPF_CGROUP_UDP4_SENDMSG,
            BPF_CGROUP_UDP6_SENDMSG, BPF_CGROUP_SYSCTL,
            BPF_CGROUP_UDP4_RECVMSG, BPF_CGROUP_UDP6_RECVMSG,
            BPF_CGROUP_GETSOCKOPT, BPF_CGROUP_SETSOCKOPT,
            BPF_CGROUP_INET_SOCK_RELEASE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ingress_is_zero() {
        assert_eq!(BPF_CGROUP_INET_INGRESS, 0);
    }

    #[test]
    fn test_attach_flags_power_of_two() {
        assert!(BPF_F_ALLOW_MULTI.is_power_of_two());
        assert!(BPF_F_REPLACE.is_power_of_two());
    }

    #[test]
    fn test_attach_flags_no_overlap() {
        assert_eq!(BPF_F_ALLOW_MULTI & BPF_F_REPLACE, 0);
    }

    #[test]
    fn test_ipv4_ipv6_pairs() {
        assert_eq!(BPF_CGROUP_INET4_BIND + 1, BPF_CGROUP_INET6_BIND);
        assert_eq!(BPF_CGROUP_INET4_CONNECT + 1, BPF_CGROUP_INET6_CONNECT);
    }
}
