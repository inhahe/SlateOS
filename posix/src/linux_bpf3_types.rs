//! `<linux/bpf.h>` — Additional BPF constants (part 3).
//!
//! Supplementary BPF constants covering attach types,
//! link types, map flags, and helper function IDs.

// ---------------------------------------------------------------------------
// BPF attach types (BPF_*)
// ---------------------------------------------------------------------------

/// Cgroup ingress.
pub const BPF_CGROUP_INET_INGRESS: u32 = 0;
/// Cgroup egress.
pub const BPF_CGROUP_INET_EGRESS: u32 = 1;
/// Cgroup sock create.
pub const BPF_CGROUP_INET_SOCK_CREATE: u32 = 2;
/// Cgroup sock ops.
pub const BPF_CGROUP_SOCK_OPS: u32 = 3;
/// SK SKB stream parser.
pub const BPF_SK_SKB_STREAM_PARSER: u32 = 4;
/// SK SKB stream verdict.
pub const BPF_SK_SKB_STREAM_VERDICT: u32 = 5;
/// Cgroup device.
pub const BPF_CGROUP_DEVICE: u32 = 6;
/// SK MSG verdict.
pub const BPF_SK_MSG_VERDICT: u32 = 7;
/// Cgroup inet4 bind.
pub const BPF_CGROUP_INET4_BIND: u32 = 8;
/// Cgroup inet6 bind.
pub const BPF_CGROUP_INET6_BIND: u32 = 9;
/// Cgroup inet4 connect.
pub const BPF_CGROUP_INET4_CONNECT: u32 = 10;
/// Cgroup inet6 connect.
pub const BPF_CGROUP_INET6_CONNECT: u32 = 11;
/// Cgroup inet4 post bind.
pub const BPF_CGROUP_INET4_POST_BIND: u32 = 12;
/// Cgroup inet6 post bind.
pub const BPF_CGROUP_INET6_POST_BIND: u32 = 13;
/// Cgroup UDP4 sendmsg.
pub const BPF_CGROUP_UDP4_SENDMSG: u32 = 14;
/// Cgroup UDP6 sendmsg.
pub const BPF_CGROUP_UDP6_SENDMSG: u32 = 15;
/// Lirc mode2.
pub const BPF_LIRC_MODE2: u32 = 16;
/// Flow dissector.
pub const BPF_FLOW_DISSECTOR: u32 = 17;
/// Cgroup sysctl.
pub const BPF_CGROUP_SYSCTL: u32 = 18;
/// Cgroup UDP4 recvmsg.
pub const BPF_CGROUP_UDP4_RECVMSG: u32 = 19;
/// Cgroup UDP6 recvmsg.
pub const BPF_CGROUP_UDP6_RECVMSG: u32 = 20;
/// Cgroup getsockopt.
pub const BPF_CGROUP_GETSOCKOPT: u32 = 21;
/// Cgroup setsockopt.
pub const BPF_CGROUP_SETSOCKOPT: u32 = 22;

// ---------------------------------------------------------------------------
// BPF link types
// ---------------------------------------------------------------------------

/// Unspecified link.
pub const BPF_LINK_TYPE_UNSPEC: u32 = 0;
/// Raw tracepoint link.
pub const BPF_LINK_TYPE_RAW_TRACEPOINT: u32 = 1;
/// Tracing link.
pub const BPF_LINK_TYPE_TRACING: u32 = 2;
/// Cgroup link.
pub const BPF_LINK_TYPE_CGROUP: u32 = 3;
/// Iter link.
pub const BPF_LINK_TYPE_ITER: u32 = 4;
/// Netns link.
pub const BPF_LINK_TYPE_NETNS: u32 = 5;
/// XDP link.
pub const BPF_LINK_TYPE_XDP: u32 = 6;
/// Perf event link.
pub const BPF_LINK_TYPE_PERF_EVENT: u32 = 7;
/// Kprobe multi link.
pub const BPF_LINK_TYPE_KPROBE_MULTI: u32 = 8;
/// Struct ops link.
pub const BPF_LINK_TYPE_STRUCT_OPS: u32 = 9;
/// Netfilter link.
pub const BPF_LINK_TYPE_NETFILTER: u32 = 10;
/// TCX link.
pub const BPF_LINK_TYPE_TCX: u32 = 11;
/// Uprobe multi link.
pub const BPF_LINK_TYPE_UPROBE_MULTI: u32 = 12;
/// Netkit link.
pub const BPF_LINK_TYPE_NETKIT: u32 = 13;

// ---------------------------------------------------------------------------
// BPF map update flags
// ---------------------------------------------------------------------------

/// Create or update.
pub const BPF_ANY: u32 = 0;
/// Create only (fail if exists).
pub const BPF_NOEXIST: u32 = 1;
/// Update only (fail if not exists).
pub const BPF_EXIST: u32 = 2;
/// Spinlock flag.
pub const BPF_F_LOCK: u32 = 4;

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
            BPF_SK_SKB_STREAM_PARSER, BPF_SK_SKB_STREAM_VERDICT,
            BPF_CGROUP_DEVICE, BPF_SK_MSG_VERDICT,
            BPF_CGROUP_INET4_BIND, BPF_CGROUP_INET6_BIND,
            BPF_CGROUP_INET4_CONNECT, BPF_CGROUP_INET6_CONNECT,
            BPF_CGROUP_INET4_POST_BIND, BPF_CGROUP_INET6_POST_BIND,
            BPF_CGROUP_UDP4_SENDMSG, BPF_CGROUP_UDP6_SENDMSG,
            BPF_LIRC_MODE2, BPF_FLOW_DISSECTOR,
            BPF_CGROUP_SYSCTL, BPF_CGROUP_UDP4_RECVMSG,
            BPF_CGROUP_UDP6_RECVMSG, BPF_CGROUP_GETSOCKOPT,
            BPF_CGROUP_SETSOCKOPT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_link_types_distinct() {
        let types = [
            BPF_LINK_TYPE_UNSPEC, BPF_LINK_TYPE_RAW_TRACEPOINT,
            BPF_LINK_TYPE_TRACING, BPF_LINK_TYPE_CGROUP,
            BPF_LINK_TYPE_ITER, BPF_LINK_TYPE_NETNS,
            BPF_LINK_TYPE_XDP, BPF_LINK_TYPE_PERF_EVENT,
            BPF_LINK_TYPE_KPROBE_MULTI, BPF_LINK_TYPE_STRUCT_OPS,
            BPF_LINK_TYPE_NETFILTER, BPF_LINK_TYPE_TCX,
            BPF_LINK_TYPE_UPROBE_MULTI, BPF_LINK_TYPE_NETKIT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_map_update_flags_distinct() {
        let flags = [BPF_ANY, BPF_NOEXIST, BPF_EXIST, BPF_F_LOCK];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
