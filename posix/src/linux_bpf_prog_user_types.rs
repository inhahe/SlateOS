//! `<linux/bpf.h>` — `enum bpf_prog_type` (BPF program kinds).
//!
//! The program type selects which verifier context, helper set, and
//! attach points apply. Each program declares its type at load
//! time (`BPF_PROG_LOAD.prog_type`).

// ---------------------------------------------------------------------------
// `enum bpf_prog_type` (dense from 0..32+)
// ---------------------------------------------------------------------------

pub const BPF_PROG_TYPE_UNSPEC: u32 = 0;
pub const BPF_PROG_TYPE_SOCKET_FILTER: u32 = 1;
pub const BPF_PROG_TYPE_KPROBE: u32 = 2;
pub const BPF_PROG_TYPE_SCHED_CLS: u32 = 3;
pub const BPF_PROG_TYPE_SCHED_ACT: u32 = 4;
pub const BPF_PROG_TYPE_TRACEPOINT: u32 = 5;
pub const BPF_PROG_TYPE_XDP: u32 = 6;
pub const BPF_PROG_TYPE_PERF_EVENT: u32 = 7;
pub const BPF_PROG_TYPE_CGROUP_SKB: u32 = 8;
pub const BPF_PROG_TYPE_CGROUP_SOCK: u32 = 9;
pub const BPF_PROG_TYPE_LWT_IN: u32 = 10;
pub const BPF_PROG_TYPE_LWT_OUT: u32 = 11;
pub const BPF_PROG_TYPE_LWT_XMIT: u32 = 12;
pub const BPF_PROG_TYPE_SOCK_OPS: u32 = 13;
pub const BPF_PROG_TYPE_SK_SKB: u32 = 14;
pub const BPF_PROG_TYPE_CGROUP_DEVICE: u32 = 15;
pub const BPF_PROG_TYPE_SK_MSG: u32 = 16;
pub const BPF_PROG_TYPE_RAW_TRACEPOINT: u32 = 17;
pub const BPF_PROG_TYPE_CGROUP_SOCK_ADDR: u32 = 18;
pub const BPF_PROG_TYPE_LWT_SEG6LOCAL: u32 = 19;
pub const BPF_PROG_TYPE_LIRC_MODE2: u32 = 20;
pub const BPF_PROG_TYPE_SK_REUSEPORT: u32 = 21;
pub const BPF_PROG_TYPE_FLOW_DISSECTOR: u32 = 22;
pub const BPF_PROG_TYPE_CGROUP_SYSCTL: u32 = 23;
pub const BPF_PROG_TYPE_RAW_TRACEPOINT_WRITABLE: u32 = 24;
pub const BPF_PROG_TYPE_CGROUP_SOCKOPT: u32 = 25;
pub const BPF_PROG_TYPE_TRACING: u32 = 26;
pub const BPF_PROG_TYPE_STRUCT_OPS: u32 = 27;
pub const BPF_PROG_TYPE_EXT: u32 = 28;
pub const BPF_PROG_TYPE_LSM: u32 = 29;
pub const BPF_PROG_TYPE_SK_LOOKUP: u32 = 30;
pub const BPF_PROG_TYPE_SYSCALL: u32 = 31;
pub const BPF_PROG_TYPE_NETFILTER: u32 = 32;

pub const __MAX_BPF_PROG_TYPE: u32 = 33;

// ---------------------------------------------------------------------------
// Program-load flags (`BPF_PROG_LOAD.prog_flags`)
// ---------------------------------------------------------------------------

pub const BPF_F_STRICT_ALIGNMENT: u32 = 1 << 0;
pub const BPF_F_ANY_ALIGNMENT: u32 = 1 << 1;
pub const BPF_F_TEST_RND_HI32: u32 = 1 << 2;
pub const BPF_F_TEST_STATE_FREQ: u32 = 1 << 3;
pub const BPF_F_SLEEPABLE: u32 = 1 << 4;
pub const BPF_F_XDP_HAS_FRAGS: u32 = 1 << 5;
pub const BPF_F_XDP_DEV_BOUND_ONLY: u32 = 1 << 6;
pub const BPF_F_TEST_REG_INVARIANTS: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prog_types_dense_0_to_32() {
        // 33 enumerators 0..=32.
        assert_eq!(BPF_PROG_TYPE_UNSPEC, 0);
        assert_eq!(BPF_PROG_TYPE_NETFILTER, 32);
        assert_eq!(__MAX_BPF_PROG_TYPE, 33);
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(BPF_PROG_TYPE_UNSPEC, 0);
        // SOCKET_FILTER is the original Linux 3.18 program type.
        assert_eq!(BPF_PROG_TYPE_SOCKET_FILTER, 1);
    }

    #[test]
    fn test_lwt_family_clustered() {
        // LWT (lightweight tunnel) program types.
        for v in [
            BPF_PROG_TYPE_LWT_IN,
            BPF_PROG_TYPE_LWT_OUT,
            BPF_PROG_TYPE_LWT_XMIT,
            BPF_PROG_TYPE_LWT_SEG6LOCAL,
        ] {
            assert!((10..=19).contains(&v));
        }
        // LWT_IN/OUT/XMIT are three contiguous codes 10..=12.
        assert_eq!(BPF_PROG_TYPE_LWT_OUT - BPF_PROG_TYPE_LWT_IN, 1);
        assert_eq!(BPF_PROG_TYPE_LWT_XMIT - BPF_PROG_TYPE_LWT_OUT, 1);
    }

    #[test]
    fn test_sched_cls_act_pair() {
        // tc classifier / action pair.
        assert_eq!(BPF_PROG_TYPE_SCHED_CLS, 3);
        assert_eq!(BPF_PROG_TYPE_SCHED_ACT, 4);
        assert_eq!(BPF_PROG_TYPE_SCHED_ACT - BPF_PROG_TYPE_SCHED_CLS, 1);
    }

    #[test]
    fn test_cgroup_family_distinct() {
        let c = [
            BPF_PROG_TYPE_CGROUP_SKB,
            BPF_PROG_TYPE_CGROUP_SOCK,
            BPF_PROG_TYPE_CGROUP_DEVICE,
            BPF_PROG_TYPE_CGROUP_SOCK_ADDR,
            BPF_PROG_TYPE_CGROUP_SYSCTL,
            BPF_PROG_TYPE_CGROUP_SOCKOPT,
        ];
        for (i, &a) in c.iter().enumerate() {
            for &b in &c[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn test_prog_flags_each_single_bit() {
        let f = [
            BPF_F_STRICT_ALIGNMENT,
            BPF_F_ANY_ALIGNMENT,
            BPF_F_TEST_RND_HI32,
            BPF_F_TEST_STATE_FREQ,
            BPF_F_SLEEPABLE,
            BPF_F_XDP_HAS_FRAGS,
            BPF_F_XDP_DEV_BOUND_ONLY,
            BPF_F_TEST_REG_INVARIANTS,
        ];
        let mut or = 0u32;
        for (i, &v) in f.iter().enumerate() {
            assert!(v.is_power_of_two());
            assert_eq!(v, 1u32 << i);
            or |= v;
        }
        // 8 contiguous low bits.
        assert_eq!(or, 0xFF);
    }

    #[test]
    fn test_strict_vs_any_alignment_opposed() {
        // STRICT and ANY are mutually-exclusive verifier modes.
        assert_eq!(BPF_F_ANY_ALIGNMENT, BPF_F_STRICT_ALIGNMENT << 1);
    }
}
