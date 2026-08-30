//! `<linux/bpf.h>` — `enum bpf_link_type` (BPF link kinds).
//!
//! A BPF link is a refcounted attach record returned by
//! `BPF_LINK_CREATE`. Closing the link's fd detaches the program.
//! Each link type corresponds to one or more attach types.

// ---------------------------------------------------------------------------
// `enum bpf_link_type` (dense from 0..14)
// ---------------------------------------------------------------------------

pub const BPF_LINK_TYPE_UNSPEC: u32 = 0;
pub const BPF_LINK_TYPE_RAW_TRACEPOINT: u32 = 1;
pub const BPF_LINK_TYPE_TRACING: u32 = 2;
pub const BPF_LINK_TYPE_CGROUP: u32 = 3;
pub const BPF_LINK_TYPE_ITER: u32 = 4;
pub const BPF_LINK_TYPE_NETNS: u32 = 5;
pub const BPF_LINK_TYPE_XDP: u32 = 6;
pub const BPF_LINK_TYPE_PERF_EVENT: u32 = 7;
pub const BPF_LINK_TYPE_KPROBE_MULTI: u32 = 8;
pub const BPF_LINK_TYPE_STRUCT_OPS: u32 = 9;
pub const BPF_LINK_TYPE_NETFILTER: u32 = 10;
pub const BPF_LINK_TYPE_TCX: u32 = 11;
pub const BPF_LINK_TYPE_UPROBE_MULTI: u32 = 12;
pub const BPF_LINK_TYPE_NETKIT: u32 = 13;

pub const __MAX_BPF_LINK_TYPE: u32 = 14;

// ---------------------------------------------------------------------------
// Iterator kinds (`enum bpf_iter_type`)
// ---------------------------------------------------------------------------

pub const BPF_ITER_TYPE_UNSPEC: u32 = 0;
pub const BPF_ITER_TYPE_BPF_MAP: u32 = 1;

// ---------------------------------------------------------------------------
// Link-create flags
// ---------------------------------------------------------------------------

pub const BPF_F_LINK_CREATE_KPROBE_RETURN: u32 = 1 << 0;
pub const BPF_F_LINK_CREATE_KPROBE_SESSION: u32 = 1 << 1;
pub const BPF_F_LINK_CREATE_UPROBE_RETURN: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_types_dense_0_to_13() {
        let l = [
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
            BPF_LINK_TYPE_NETFILTER,
            BPF_LINK_TYPE_TCX,
            BPF_LINK_TYPE_UPROBE_MULTI,
            BPF_LINK_TYPE_NETKIT,
        ];
        for (i, &v) in l.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(__MAX_BPF_LINK_TYPE as usize, l.len());
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(BPF_LINK_TYPE_UNSPEC, 0);
        assert_eq!(BPF_ITER_TYPE_UNSPEC, 0);
    }

    #[test]
    fn test_iter_types_dense() {
        assert_eq!(BPF_ITER_TYPE_BPF_MAP, 1);
    }

    #[test]
    fn test_multi_probe_types() {
        // kprobe_multi / uprobe_multi sit at 8 / 12 — non-adjacent, as
        // they were added across separate kernel releases.
        assert_eq!(BPF_LINK_TYPE_KPROBE_MULTI, 8);
        assert_eq!(BPF_LINK_TYPE_UPROBE_MULTI, 12);
    }

    #[test]
    fn test_link_flags_single_bits() {
        for &v in &[
            BPF_F_LINK_CREATE_KPROBE_RETURN,
            BPF_F_LINK_CREATE_KPROBE_SESSION,
            BPF_F_LINK_CREATE_UPROBE_RETURN,
        ] {
            assert!(v.is_power_of_two());
        }
        // The kprobe-return and uprobe-return flags reuse bit 0 — they
        // gate distinct link types, so the overlap is harmless.
        assert_eq!(
            BPF_F_LINK_CREATE_KPROBE_RETURN,
            BPF_F_LINK_CREATE_UPROBE_RETURN
        );
        // kprobe-session is bit 1.
        assert_eq!(BPF_F_LINK_CREATE_KPROBE_SESSION, 1 << 1);
    }
}
