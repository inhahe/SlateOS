//! `<linux/bpf.h>` — Additional BPF constants (batch 4).
//!
//! Supplementary BPF constants covering BPF link types,
//! BPF iterator types, and BPF stats commands.

// ---------------------------------------------------------------------------
// BPF link types (BPF_LINK_TYPE_*)
// ---------------------------------------------------------------------------

/// Unspecified link type.
pub const BPF_LINK_TYPE_UNSPEC: u32 = 0;
/// Raw tracepoint link.
pub const BPF_LINK_TYPE_RAW_TRACEPOINT: u32 = 1;
/// Tracing (fentry/fexit/fmod_ret) link.
pub const BPF_LINK_TYPE_TRACING: u32 = 2;
/// Cgroup link.
pub const BPF_LINK_TYPE_CGROUP: u32 = 3;
/// Iterator link.
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
/// Tcx link.
pub const BPF_LINK_TYPE_TCX: u32 = 11;
/// Uprobe multi link.
pub const BPF_LINK_TYPE_UPROBE_MULTI: u32 = 12;
/// Netkit link.
pub const BPF_LINK_TYPE_NETKIT: u32 = 13;

// ---------------------------------------------------------------------------
// BPF iterator types
// ---------------------------------------------------------------------------

/// Iterate over map elements.
pub const BPF_ITER_MAP_ELEM: u32 = 0;
/// Iterate over tasks.
pub const BPF_ITER_TASK: u32 = 1;
/// Iterate over task files.
pub const BPF_ITER_TASK_FILE: u32 = 2;
/// Iterate over VMA entries.
pub const BPF_ITER_TASK_VMA: u32 = 3;
/// Iterate over BPF programs.
pub const BPF_ITER_BPROG: u32 = 4;
/// Iterate over BPF maps.
pub const BPF_ITER_BMAP: u32 = 5;

// ---------------------------------------------------------------------------
// BPF stats commands
// ---------------------------------------------------------------------------

/// Enable stats collection.
pub const BPF_STATS_ENABLE: u32 = 0;
/// Disable stats collection.
pub const BPF_STATS_DISABLE: u32 = 1;

/// Run count stat type.
pub const BPF_STATS_RUN_CNT: u32 = 0;
/// Run time stat type.
pub const BPF_STATS_RUN_TIME: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_link_type_values() {
        assert_eq!(BPF_LINK_TYPE_UNSPEC, 0);
        assert_eq!(BPF_LINK_TYPE_NETKIT, 13);
    }

    #[test]
    fn test_iter_types_distinct() {
        let types = [
            BPF_ITER_MAP_ELEM, BPF_ITER_TASK,
            BPF_ITER_TASK_FILE, BPF_ITER_TASK_VMA,
            BPF_ITER_BPROG, BPF_ITER_BMAP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_stats_commands_distinct() {
        assert_ne!(BPF_STATS_ENABLE, BPF_STATS_DISABLE);
    }

    #[test]
    fn test_stats_types_distinct() {
        assert_ne!(BPF_STATS_RUN_CNT, BPF_STATS_RUN_TIME);
    }
}
