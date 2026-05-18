//! `<linux/bpf.h>` — BPF link type and flag constants.
//!
//! BPF links provide a stable reference between a BPF program
//! and its attachment point. They support atomic replacement,
//! detach, and introspection operations.

// ---------------------------------------------------------------------------
// BPF link types (enum bpf_link_type)
// ---------------------------------------------------------------------------

/// Unspecified link type.
pub const BPF_LINK_TYPE_UNSPEC: u32 = 0;
/// Raw tracepoint link.
pub const BPF_LINK_TYPE_RAW_TRACEPOINT: u32 = 1;
/// Tracing link (fentry/fexit).
pub const BPF_LINK_TYPE_TRACING: u32 = 2;
/// cgroup link.
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
/// TC (traffic control) link.
pub const BPF_LINK_TYPE_TCX: u32 = 11;
/// Uprobe multi link.
pub const BPF_LINK_TYPE_UPROBE_MULTI: u32 = 12;
/// Netkit link.
pub const BPF_LINK_TYPE_NETKIT: u32 = 13;

// ---------------------------------------------------------------------------
// BPF link flags
// ---------------------------------------------------------------------------

/// Allow the link to pin to bpffs.
pub const BPF_F_LINK_PIN: u32 = 1 << 0;
/// Allow the link to be updated atomically.
pub const BPF_F_LINK_UPDATE: u32 = 1 << 1;
/// Create link without attaching.
pub const BPF_F_LINK_DETACH: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// BPF link update flags
// ---------------------------------------------------------------------------

/// Replace the old program if it matches.
pub const BPF_F_REPLACE: u32 = 1 << 2;
/// Update flags: before a specific link.
pub const BPF_F_BEFORE: u32 = 1 << 3;
/// Update flags: after a specific link.
pub const BPF_F_AFTER: u32 = 1 << 4;
/// Update flags: program ID check.
pub const BPF_F_ID: u32 = 1 << 5;
/// Update flags: link-specific ordering.
pub const BPF_F_LINK: u32 = 1 << 13;

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
    fn test_unspec_is_zero() {
        assert_eq!(BPF_LINK_TYPE_UNSPEC, 0);
    }

    #[test]
    fn test_update_flags_no_overlap() {
        let flags = [BPF_F_REPLACE, BPF_F_BEFORE, BPF_F_AFTER, BPF_F_ID, BPF_F_LINK];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_update_flags_power_of_two() {
        assert!(BPF_F_REPLACE.is_power_of_two());
        assert!(BPF_F_BEFORE.is_power_of_two());
        assert!(BPF_F_AFTER.is_power_of_two());
        assert!(BPF_F_ID.is_power_of_two());
        assert!(BPF_F_LINK.is_power_of_two());
    }

    #[test]
    fn test_xdp_link_type() {
        assert_eq!(BPF_LINK_TYPE_XDP, 6);
    }
}
