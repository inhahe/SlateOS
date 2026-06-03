//! `<linux/bpf.h>` — BPF attachment point type constants.
//!
//! BPF programs are attached to specific kernel hooks. These
//! constants enumerate the attachment points where eBPF programs
//! can be loaded and executed.

// ---------------------------------------------------------------------------
// BPF attach types (enum bpf_attach_type)
// ---------------------------------------------------------------------------

/// cgroup inet ingress.
pub const BPF_CGROUP_INET_INGRESS: u32 = 0;
/// cgroup inet egress.
pub const BPF_CGROUP_INET_EGRESS: u32 = 1;
/// cgroup inet socket create.
pub const BPF_CGROUP_INET_SOCK_CREATE: u32 = 2;
/// cgroup socket operations.
pub const BPF_CGROUP_SOCK_OPS: u32 = 3;
/// SK SKB stream parser.
pub const BPF_SK_SKB_STREAM_PARSER: u32 = 4;
/// SK SKB stream verdict.
pub const BPF_SK_SKB_STREAM_VERDICT: u32 = 5;
/// cgroup device.
pub const BPF_CGROUP_DEVICE: u32 = 6;
/// SK message verdict.
pub const BPF_SK_MSG_VERDICT: u32 = 7;
/// cgroup inet4 bind.
pub const BPF_CGROUP_INET4_BIND: u32 = 8;
/// cgroup inet6 bind.
pub const BPF_CGROUP_INET6_BIND: u32 = 9;
/// cgroup inet4 connect.
pub const BPF_CGROUP_INET4_CONNECT: u32 = 10;
/// cgroup inet6 connect.
pub const BPF_CGROUP_INET6_CONNECT: u32 = 11;
/// cgroup inet4 post bind.
pub const BPF_CGROUP_INET4_POST_BIND: u32 = 12;
/// cgroup inet6 post bind.
pub const BPF_CGROUP_INET6_POST_BIND: u32 = 13;
/// cgroup UDP4 sendmsg.
pub const BPF_CGROUP_UDP4_SENDMSG: u32 = 14;
/// cgroup UDP6 sendmsg.
pub const BPF_CGROUP_UDP6_SENDMSG: u32 = 15;
/// lirc mode2.
pub const BPF_LIRC_MODE2: u32 = 16;
/// flow dissector.
pub const BPF_FLOW_DISSECTOR: u32 = 17;
/// cgroup sysctl.
pub const BPF_CGROUP_SYSCTL: u32 = 18;
/// cgroup UDP4 recvmsg.
pub const BPF_CGROUP_UDP4_RECVMSG: u32 = 19;
/// cgroup UDP6 recvmsg.
pub const BPF_CGROUP_UDP6_RECVMSG: u32 = 20;
/// cgroup getsockopt.
pub const BPF_CGROUP_GETSOCKOPT: u32 = 21;
/// cgroup setsockopt.
pub const BPF_CGROUP_SETSOCKOPT: u32 = 22;
/// Trace raw tracepoint writable.
pub const BPF_TRACE_RAW_TP: u32 = 23;
/// Trace fentry (function entry).
pub const BPF_TRACE_FENTRY: u32 = 24;
/// Trace fexit (function exit).
pub const BPF_TRACE_FEXIT: u32 = 25;
/// Modify return value.
pub const BPF_MODIFY_RETURN: u32 = 26;
/// LSM MAC hook.
pub const BPF_LSM_MAC: u32 = 27;
/// Trace iterator.
pub const BPF_TRACE_ITER: u32 = 28;
/// cgroup inet4 getpeername.
pub const BPF_CGROUP_INET4_GETPEERNAME: u32 = 29;
/// cgroup inet6 getpeername.
pub const BPF_CGROUP_INET6_GETPEERNAME: u32 = 30;
/// cgroup inet4 getsockname.
pub const BPF_CGROUP_INET4_GETSOCKNAME: u32 = 31;
/// cgroup inet6 getsockname.
pub const BPF_CGROUP_INET6_GETSOCKNAME: u32 = 32;
/// XDP device map.
pub const BPF_XDP_DEVMAP: u32 = 33;
/// cgroup inet socket release.
pub const BPF_CGROUP_INET_SOCK_RELEASE: u32 = 34;
/// XDP CPU map.
pub const BPF_XDP_CPUMAP: u32 = 35;
/// SK lookup.
pub const BPF_SK_LOOKUP: u32 = 36;
/// XDP (generic attach).
pub const BPF_XDP: u32 = 37;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attach_types_distinct() {
        let types = [
            BPF_CGROUP_INET_INGRESS,
            BPF_CGROUP_INET_EGRESS,
            BPF_CGROUP_INET_SOCK_CREATE,
            BPF_CGROUP_SOCK_OPS,
            BPF_SK_SKB_STREAM_PARSER,
            BPF_SK_SKB_STREAM_VERDICT,
            BPF_CGROUP_DEVICE,
            BPF_SK_MSG_VERDICT,
            BPF_CGROUP_INET4_BIND,
            BPF_CGROUP_INET6_BIND,
            BPF_CGROUP_INET4_CONNECT,
            BPF_CGROUP_INET6_CONNECT,
            BPF_CGROUP_INET4_POST_BIND,
            BPF_CGROUP_INET6_POST_BIND,
            BPF_CGROUP_UDP4_SENDMSG,
            BPF_CGROUP_UDP6_SENDMSG,
            BPF_LIRC_MODE2,
            BPF_FLOW_DISSECTOR,
            BPF_CGROUP_SYSCTL,
            BPF_CGROUP_UDP4_RECVMSG,
            BPF_CGROUP_UDP6_RECVMSG,
            BPF_CGROUP_GETSOCKOPT,
            BPF_CGROUP_SETSOCKOPT,
            BPF_TRACE_RAW_TP,
            BPF_TRACE_FENTRY,
            BPF_TRACE_FEXIT,
            BPF_MODIFY_RETURN,
            BPF_LSM_MAC,
            BPF_TRACE_ITER,
            BPF_CGROUP_INET4_GETPEERNAME,
            BPF_CGROUP_INET6_GETPEERNAME,
            BPF_CGROUP_INET4_GETSOCKNAME,
            BPF_CGROUP_INET6_GETSOCKNAME,
            BPF_XDP_DEVMAP,
            BPF_CGROUP_INET_SOCK_RELEASE,
            BPF_XDP_CPUMAP,
            BPF_SK_LOOKUP,
            BPF_XDP,
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
    fn test_fentry_fexit() {
        assert_eq!(BPF_TRACE_FENTRY, 24);
        assert_eq!(BPF_TRACE_FEXIT, 25);
    }
}
