//! `<linux/bpf.h>` (trace attach subset) — BPF tracing attachment constants.
//!
//! BPF programs can attach to various kernel tracing points: kprobes,
//! tracepoints, perf events, fentry/fexit (optimised function entry/
//! exit), and LSM hooks. The attach type determines where the BPF
//! program runs and what context it receives.

// ---------------------------------------------------------------------------
// BPF trace attach types (bpf_attach_type subset)
// ---------------------------------------------------------------------------

/// Attach to raw tracepoint.
pub const BPF_TRACE_RAW_TP: u32 = 17;
/// Attach to fentry (function entry, no kprobe overhead).
pub const BPF_TRACE_FENTRY: u32 = 24;
/// Attach to fexit (function return, no kretprobe overhead).
pub const BPF_TRACE_FEXIT: u32 = 25;
/// Attach to modify function return value.
pub const BPF_MODIFY_RETURN: u32 = 26;
/// Attach to LSM hook.
pub const BPF_LSM_MAC: u32 = 27;
/// Attach as tracing iterator.
pub const BPF_TRACE_ITER: u32 = 28;
/// Attach to cgroup inet ingress.
pub const BPF_LSM_CGROUP: u32 = 38;
/// Attach to kprobe multi (batch).
pub const BPF_TRACE_KPROBE_MULTI: u32 = 42;
/// Attach to uprobe multi (batch).
pub const BPF_TRACE_UPROBE_MULTI: u32 = 43;

// ---------------------------------------------------------------------------
// BPF link types (bpf_link_type)
// ---------------------------------------------------------------------------

/// Unspecified link.
pub const BPF_LINK_TYPE_UNSPEC: u32 = 0;
/// Raw tracepoint link.
pub const BPF_LINK_TYPE_RAW_TRACEPOINT: u32 = 1;
/// Tracing link (fentry/fexit/modify_return).
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
/// Uprobe multi link.
pub const BPF_LINK_TYPE_UPROBE_MULTI: u32 = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attach_types_distinct() {
        let types = [
            BPF_TRACE_RAW_TP,
            BPF_TRACE_FENTRY,
            BPF_TRACE_FEXIT,
            BPF_MODIFY_RETURN,
            BPF_LSM_MAC,
            BPF_TRACE_ITER,
            BPF_LSM_CGROUP,
            BPF_TRACE_KPROBE_MULTI,
            BPF_TRACE_UPROBE_MULTI,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_link_types_distinct() {
        let links = [
            BPF_LINK_TYPE_UNSPEC,
            BPF_LINK_TYPE_RAW_TRACEPOINT,
            BPF_LINK_TYPE_TRACING,
            BPF_LINK_TYPE_CGROUP,
            BPF_LINK_TYPE_ITER,
            BPF_LINK_TYPE_NETNS,
            BPF_LINK_TYPE_XDP,
            BPF_LINK_TYPE_PERF_EVENT,
            BPF_LINK_TYPE_KPROBE_MULTI,
            BPF_LINK_TYPE_STRUCT_OPS,
            BPF_LINK_TYPE_UPROBE_MULTI,
        ];
        for i in 0..links.len() {
            for j in (i + 1)..links.len() {
                assert_ne!(links[i], links[j]);
            }
        }
    }

    #[test]
    fn test_fentry_fexit_adjacent() {
        assert_eq!(BPF_TRACE_FEXIT, BPF_TRACE_FENTRY + 1);
    }
}
