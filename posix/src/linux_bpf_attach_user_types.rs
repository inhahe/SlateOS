//! `<linux/bpf.h>` — `BPF_PROG_ATTACH` / `BPF_PROG_DETACH` attach
//! types and attach flags.
//!
//! Attach types fan out by program subsystem (cgroup, XDP, tracing,
//! lsm, etc.). The dense numeric enum is what userspace passes in
//! `union bpf_attr.target_fd / attach_type`, and the matching
//! attach-flag bitmask controls multi-attach behavior.

// ---------------------------------------------------------------------------
// `enum bpf_attach_type` (dense from 0..50+)
// ---------------------------------------------------------------------------

pub const BPF_CGROUP_INET_INGRESS: u32 = 0;
pub const BPF_CGROUP_INET_EGRESS: u32 = 1;
pub const BPF_CGROUP_INET_SOCK_CREATE: u32 = 2;
pub const BPF_CGROUP_SOCK_OPS: u32 = 3;
pub const BPF_SK_SKB_STREAM_PARSER: u32 = 4;
pub const BPF_SK_SKB_STREAM_VERDICT: u32 = 5;
pub const BPF_CGROUP_DEVICE: u32 = 6;
pub const BPF_SK_MSG_VERDICT: u32 = 7;
pub const BPF_CGROUP_INET4_BIND: u32 = 8;
pub const BPF_CGROUP_INET6_BIND: u32 = 9;
pub const BPF_CGROUP_INET4_CONNECT: u32 = 10;
pub const BPF_CGROUP_INET6_CONNECT: u32 = 11;
pub const BPF_CGROUP_INET4_POST_BIND: u32 = 12;
pub const BPF_CGROUP_INET6_POST_BIND: u32 = 13;
pub const BPF_CGROUP_UDP4_SENDMSG: u32 = 14;
pub const BPF_CGROUP_UDP6_SENDMSG: u32 = 15;
pub const BPF_LIRC_MODE2: u32 = 16;
pub const BPF_FLOW_DISSECTOR: u32 = 17;
pub const BPF_CGROUP_SYSCTL: u32 = 18;
pub const BPF_CGROUP_UDP4_RECVMSG: u32 = 19;
pub const BPF_CGROUP_UDP6_RECVMSG: u32 = 20;
pub const BPF_CGROUP_GETSOCKOPT: u32 = 21;
pub const BPF_CGROUP_SETSOCKOPT: u32 = 22;
pub const BPF_TRACE_RAW_TP: u32 = 23;
pub const BPF_TRACE_FENTRY: u32 = 24;
pub const BPF_TRACE_FEXIT: u32 = 25;
pub const BPF_MODIFY_RETURN: u32 = 26;
pub const BPF_LSM_MAC: u32 = 27;
pub const BPF_TRACE_ITER: u32 = 28;
pub const BPF_CGROUP_INET4_GETPEERNAME: u32 = 29;
pub const BPF_CGROUP_INET6_GETPEERNAME: u32 = 30;
pub const BPF_CGROUP_INET4_GETSOCKNAME: u32 = 31;
pub const BPF_CGROUP_INET6_GETSOCKNAME: u32 = 32;
pub const BPF_XDP_DEVMAP: u32 = 33;
pub const BPF_CGROUP_INET_SOCK_RELEASE: u32 = 34;
pub const BPF_XDP_CPUMAP: u32 = 35;
pub const BPF_SK_LOOKUP: u32 = 36;
pub const BPF_XDP: u32 = 37;
pub const BPF_SK_SKB_VERDICT: u32 = 38;
pub const BPF_SK_REUSEPORT_SELECT: u32 = 39;
pub const BPF_SK_REUSEPORT_SELECT_OR_MIGRATE: u32 = 40;
pub const BPF_PERF_EVENT: u32 = 41;
pub const BPF_TRACE_KPROBE_MULTI: u32 = 42;
pub const BPF_LSM_CGROUP: u32 = 43;
pub const BPF_STRUCT_OPS: u32 = 44;
pub const BPF_NETFILTER: u32 = 45;
pub const BPF_TCX_INGRESS: u32 = 46;
pub const BPF_TCX_EGRESS: u32 = 47;
pub const BPF_TRACE_UPROBE_MULTI: u32 = 48;
pub const BPF_CGROUP_UNIX_CONNECT: u32 = 49;
pub const BPF_CGROUP_UNIX_SENDMSG: u32 = 50;

pub const __MAX_BPF_ATTACH_TYPE: u32 = 51;

// ---------------------------------------------------------------------------
// Attach flags (`BPF_F_ATTACH_*`)
// ---------------------------------------------------------------------------

pub const BPF_F_ALLOW_OVERRIDE: u32 = 1 << 0;
pub const BPF_F_ALLOW_MULTI: u32 = 1 << 1;
pub const BPF_F_REPLACE: u32 = 1 << 2;
pub const BPF_F_BEFORE: u32 = 1 << 3;
pub const BPF_F_AFTER: u32 = 1 << 4;
pub const BPF_F_ID: u32 = 1 << 5;
pub const BPF_F_LINK: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attach_types_dense_0_to_50() {
        let last = BPF_CGROUP_UNIX_SENDMSG;
        // 51 enumerators 0..=50.
        assert_eq!(last, 50);
        assert_eq!(__MAX_BPF_ATTACH_TYPE, 51);
    }

    #[test]
    fn test_cgroup_inet_ingress_egress_pair() {
        assert_eq!(BPF_CGROUP_INET_INGRESS, 0);
        assert_eq!(BPF_CGROUP_INET_EGRESS, 1);
        assert_eq!(
            BPF_CGROUP_INET_EGRESS - BPF_CGROUP_INET_INGRESS,
            1
        );
    }

    #[test]
    fn test_inet4_inet6_pairs_offset_by_one() {
        // Each v4/v6 attach pair sits at consecutive indices.
        for (v4, v6) in [
            (BPF_CGROUP_INET4_BIND, BPF_CGROUP_INET6_BIND),
            (BPF_CGROUP_INET4_CONNECT, BPF_CGROUP_INET6_CONNECT),
            (BPF_CGROUP_INET4_POST_BIND, BPF_CGROUP_INET6_POST_BIND),
            (BPF_CGROUP_UDP4_SENDMSG, BPF_CGROUP_UDP6_SENDMSG),
            (BPF_CGROUP_UDP4_RECVMSG, BPF_CGROUP_UDP6_RECVMSG),
            (BPF_CGROUP_INET4_GETPEERNAME, BPF_CGROUP_INET6_GETPEERNAME),
            (BPF_CGROUP_INET4_GETSOCKNAME, BPF_CGROUP_INET6_GETSOCKNAME),
        ] {
            assert_eq!(v6 - v4, 1, "v4 {v4} v6 {v6}");
        }
    }

    #[test]
    fn test_tcx_ingress_egress_pair() {
        assert_eq!(BPF_TCX_INGRESS, 46);
        assert_eq!(BPF_TCX_EGRESS, 47);
        assert_eq!(BPF_TCX_EGRESS - BPF_TCX_INGRESS, 1);
    }

    #[test]
    fn test_trace_family_clustered() {
        // The four trace attach types sit in a contiguous block.
        for v in [
            BPF_TRACE_RAW_TP,
            BPF_TRACE_FENTRY,
            BPF_TRACE_FEXIT,
            BPF_TRACE_ITER,
        ] {
            assert!((23..=28).contains(&v));
        }
    }

    #[test]
    fn test_attach_flags_each_single_bit() {
        let f = [
            BPF_F_ALLOW_OVERRIDE,
            BPF_F_ALLOW_MULTI,
            BPF_F_REPLACE,
            BPF_F_BEFORE,
            BPF_F_AFTER,
            BPF_F_ID,
            BPF_F_LINK,
        ];
        for &v in &f {
            assert!(v.is_power_of_two());
        }
        // F_LINK sits at bit 13 (intentional gap for future flags).
        assert_eq!(BPF_F_LINK, 1 << 13);
    }
}
