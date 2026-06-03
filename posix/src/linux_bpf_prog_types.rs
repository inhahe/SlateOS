//! `<linux/bpf.h>` — BPF program type constants.
//!
//! Each BPF program has a type that determines the context it
//! runs in and the helper functions available to it. These
//! constants enumerate all program types.

// ---------------------------------------------------------------------------
// BPF program types (enum bpf_prog_type)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const BPF_PROG_TYPE_UNSPEC: u32 = 0;
/// Socket filter.
pub const BPF_PROG_TYPE_SOCKET_FILTER: u32 = 1;
/// Kprobe/uprobe.
pub const BPF_PROG_TYPE_KPROBE: u32 = 2;
/// Scheduler classifier (TC).
pub const BPF_PROG_TYPE_SCHED_CLS: u32 = 3;
/// Scheduler action (TC).
pub const BPF_PROG_TYPE_SCHED_ACT: u32 = 4;
/// Tracepoint.
pub const BPF_PROG_TYPE_TRACEPOINT: u32 = 5;
/// XDP (eXpress Data Path).
pub const BPF_PROG_TYPE_XDP: u32 = 6;
/// Perf event.
pub const BPF_PROG_TYPE_PERF_EVENT: u32 = 7;
/// cgroup SKB (socket buffer).
pub const BPF_PROG_TYPE_CGROUP_SKB: u32 = 8;
/// cgroup socket.
pub const BPF_PROG_TYPE_CGROUP_SOCK: u32 = 9;
/// Lightweight tunnel encap.
pub const BPF_PROG_TYPE_LWT_IN: u32 = 10;
/// Lightweight tunnel decap.
pub const BPF_PROG_TYPE_LWT_OUT: u32 = 11;
/// Lightweight tunnel transmit.
pub const BPF_PROG_TYPE_LWT_XMIT: u32 = 12;
/// Socket operations.
pub const BPF_PROG_TYPE_SOCK_OPS: u32 = 13;
/// SK SKB.
pub const BPF_PROG_TYPE_SK_SKB: u32 = 14;
/// cgroup device.
pub const BPF_PROG_TYPE_CGROUP_DEVICE: u32 = 15;
/// SK message.
pub const BPF_PROG_TYPE_SK_MSG: u32 = 16;
/// Raw tracepoint.
pub const BPF_PROG_TYPE_RAW_TRACEPOINT: u32 = 17;
/// cgroup socket address.
pub const BPF_PROG_TYPE_CGROUP_SOCK_ADDR: u32 = 18;
/// LWT seg6local.
pub const BPF_PROG_TYPE_LWT_SEG6LOCAL: u32 = 19;
/// lirc mode2.
pub const BPF_PROG_TYPE_LIRC_MODE2: u32 = 20;
/// SK reuseport.
pub const BPF_PROG_TYPE_SK_REUSEPORT: u32 = 21;
/// Flow dissector.
pub const BPF_PROG_TYPE_FLOW_DISSECTOR: u32 = 22;
/// cgroup sysctl.
pub const BPF_PROG_TYPE_CGROUP_SYSCTL: u32 = 23;
/// Raw tracepoint writable.
pub const BPF_PROG_TYPE_RAW_TRACEPOINT_WRITABLE: u32 = 24;
/// cgroup sockopt.
pub const BPF_PROG_TYPE_CGROUP_SOCKOPT: u32 = 25;
/// Tracing (fentry/fexit/fmod_ret).
pub const BPF_PROG_TYPE_TRACING: u32 = 26;
/// Struct ops.
pub const BPF_PROG_TYPE_STRUCT_OPS: u32 = 27;
/// Extension (freplace).
pub const BPF_PROG_TYPE_EXT: u32 = 28;
/// LSM (Linux Security Module).
pub const BPF_PROG_TYPE_LSM: u32 = 29;
/// SK lookup.
pub const BPF_PROG_TYPE_SK_LOOKUP: u32 = 30;
/// Syscall.
pub const BPF_PROG_TYPE_SYSCALL: u32 = 31;
/// Netfilter.
pub const BPF_PROG_TYPE_NETFILTER: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prog_types_distinct() {
        let types = [
            BPF_PROG_TYPE_UNSPEC,
            BPF_PROG_TYPE_SOCKET_FILTER,
            BPF_PROG_TYPE_KPROBE,
            BPF_PROG_TYPE_SCHED_CLS,
            BPF_PROG_TYPE_SCHED_ACT,
            BPF_PROG_TYPE_TRACEPOINT,
            BPF_PROG_TYPE_XDP,
            BPF_PROG_TYPE_PERF_EVENT,
            BPF_PROG_TYPE_CGROUP_SKB,
            BPF_PROG_TYPE_CGROUP_SOCK,
            BPF_PROG_TYPE_LWT_IN,
            BPF_PROG_TYPE_LWT_OUT,
            BPF_PROG_TYPE_LWT_XMIT,
            BPF_PROG_TYPE_SOCK_OPS,
            BPF_PROG_TYPE_SK_SKB,
            BPF_PROG_TYPE_CGROUP_DEVICE,
            BPF_PROG_TYPE_SK_MSG,
            BPF_PROG_TYPE_RAW_TRACEPOINT,
            BPF_PROG_TYPE_CGROUP_SOCK_ADDR,
            BPF_PROG_TYPE_LWT_SEG6LOCAL,
            BPF_PROG_TYPE_LIRC_MODE2,
            BPF_PROG_TYPE_SK_REUSEPORT,
            BPF_PROG_TYPE_FLOW_DISSECTOR,
            BPF_PROG_TYPE_CGROUP_SYSCTL,
            BPF_PROG_TYPE_RAW_TRACEPOINT_WRITABLE,
            BPF_PROG_TYPE_CGROUP_SOCKOPT,
            BPF_PROG_TYPE_TRACING,
            BPF_PROG_TYPE_STRUCT_OPS,
            BPF_PROG_TYPE_EXT,
            BPF_PROG_TYPE_LSM,
            BPF_PROG_TYPE_SK_LOOKUP,
            BPF_PROG_TYPE_SYSCALL,
            BPF_PROG_TYPE_NETFILTER,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(BPF_PROG_TYPE_UNSPEC, 0);
    }

    #[test]
    fn test_xdp_type() {
        assert_eq!(BPF_PROG_TYPE_XDP, 6);
    }

    #[test]
    fn test_tracing_type() {
        assert_eq!(BPF_PROG_TYPE_TRACING, 26);
    }
}
