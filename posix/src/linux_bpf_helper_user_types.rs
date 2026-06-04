//! `<linux/bpf.h>` — BPF helper-function IDs (`enum bpf_func_id`).
//!
//! Helpers are kernel-side functions BPF programs may call. The
//! verifier maps each `call <id>` instruction to a specific kernel
//! function based on the helper ID. Only a small core subset is
//! covered here — the ones libbpf, bpftrace, and tc-bpf use.

// ---------------------------------------------------------------------------
// Core helpers (dense 0..40)
// ---------------------------------------------------------------------------

pub const BPF_FUNC_UNSPEC: u32 = 0;
pub const BPF_FUNC_MAP_LOOKUP_ELEM: u32 = 1;
pub const BPF_FUNC_MAP_UPDATE_ELEM: u32 = 2;
pub const BPF_FUNC_MAP_DELETE_ELEM: u32 = 3;
pub const BPF_FUNC_PROBE_READ: u32 = 4;
pub const BPF_FUNC_KTIME_GET_NS: u32 = 5;
pub const BPF_FUNC_TRACE_PRINTK: u32 = 6;
pub const BPF_FUNC_GET_PRANDOM_U32: u32 = 7;
pub const BPF_FUNC_GET_SMP_PROCESSOR_ID: u32 = 8;
pub const BPF_FUNC_SKB_STORE_BYTES: u32 = 9;
pub const BPF_FUNC_L3_CSUM_REPLACE: u32 = 10;
pub const BPF_FUNC_L4_CSUM_REPLACE: u32 = 11;
pub const BPF_FUNC_TAIL_CALL: u32 = 12;
pub const BPF_FUNC_CLONE_REDIRECT: u32 = 13;
pub const BPF_FUNC_GET_CURRENT_PID_TGID: u32 = 14;
pub const BPF_FUNC_GET_CURRENT_UID_GID: u32 = 15;
pub const BPF_FUNC_GET_CURRENT_COMM: u32 = 16;
pub const BPF_FUNC_GET_CGROUP_CLASSID: u32 = 17;
pub const BPF_FUNC_SKB_VLAN_PUSH: u32 = 18;
pub const BPF_FUNC_SKB_VLAN_POP: u32 = 19;
pub const BPF_FUNC_SKB_GET_TUNNEL_KEY: u32 = 20;
pub const BPF_FUNC_SKB_SET_TUNNEL_KEY: u32 = 21;
pub const BPF_FUNC_PERF_EVENT_READ: u32 = 22;
pub const BPF_FUNC_REDIRECT: u32 = 23;
pub const BPF_FUNC_GET_ROUTE_REALM: u32 = 24;
pub const BPF_FUNC_PERF_EVENT_OUTPUT: u32 = 25;
pub const BPF_FUNC_SKB_LOAD_BYTES: u32 = 26;
pub const BPF_FUNC_GET_STACKID: u32 = 27;
pub const BPF_FUNC_CSUM_DIFF: u32 = 28;
pub const BPF_FUNC_SKB_GET_TUNNEL_OPT: u32 = 29;
pub const BPF_FUNC_SKB_SET_TUNNEL_OPT: u32 = 30;
pub const BPF_FUNC_SKB_CHANGE_PROTO: u32 = 31;
pub const BPF_FUNC_SKB_CHANGE_TYPE: u32 = 32;
pub const BPF_FUNC_SKB_UNDER_CGROUP: u32 = 33;
pub const BPF_FUNC_GET_HASH_RECALC: u32 = 34;
pub const BPF_FUNC_GET_CURRENT_TASK: u32 = 35;
pub const BPF_FUNC_PROBE_WRITE_USER: u32 = 36;
pub const BPF_FUNC_CURRENT_TASK_UNDER_CGROUP: u32 = 37;
pub const BPF_FUNC_SKB_CHANGE_TAIL: u32 = 38;
pub const BPF_FUNC_SKB_PULL_DATA: u32 = 39;
pub const BPF_FUNC_CSUM_UPDATE: u32 = 40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unspec_at_zero() {
        // Helper IDs are 1-based; 0 is the "unspec" sentinel.
        assert_eq!(BPF_FUNC_UNSPEC, 0);
        // First real helper.
        assert_eq!(BPF_FUNC_MAP_LOOKUP_ELEM, 1);
    }

    #[test]
    fn test_map_helpers_dense_1_to_3() {
        let m = [
            BPF_FUNC_MAP_LOOKUP_ELEM,
            BPF_FUNC_MAP_UPDATE_ELEM,
            BPF_FUNC_MAP_DELETE_ELEM,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_helpers_dense_0_to_40() {
        let h = [
            BPF_FUNC_UNSPEC,
            BPF_FUNC_MAP_LOOKUP_ELEM,
            BPF_FUNC_MAP_UPDATE_ELEM,
            BPF_FUNC_MAP_DELETE_ELEM,
            BPF_FUNC_PROBE_READ,
            BPF_FUNC_KTIME_GET_NS,
            BPF_FUNC_TRACE_PRINTK,
            BPF_FUNC_GET_PRANDOM_U32,
            BPF_FUNC_GET_SMP_PROCESSOR_ID,
            BPF_FUNC_SKB_STORE_BYTES,
            BPF_FUNC_L3_CSUM_REPLACE,
            BPF_FUNC_L4_CSUM_REPLACE,
            BPF_FUNC_TAIL_CALL,
            BPF_FUNC_CLONE_REDIRECT,
            BPF_FUNC_GET_CURRENT_PID_TGID,
            BPF_FUNC_GET_CURRENT_UID_GID,
            BPF_FUNC_GET_CURRENT_COMM,
            BPF_FUNC_GET_CGROUP_CLASSID,
            BPF_FUNC_SKB_VLAN_PUSH,
            BPF_FUNC_SKB_VLAN_POP,
            BPF_FUNC_SKB_GET_TUNNEL_KEY,
            BPF_FUNC_SKB_SET_TUNNEL_KEY,
            BPF_FUNC_PERF_EVENT_READ,
            BPF_FUNC_REDIRECT,
            BPF_FUNC_GET_ROUTE_REALM,
            BPF_FUNC_PERF_EVENT_OUTPUT,
            BPF_FUNC_SKB_LOAD_BYTES,
            BPF_FUNC_GET_STACKID,
            BPF_FUNC_CSUM_DIFF,
            BPF_FUNC_SKB_GET_TUNNEL_OPT,
            BPF_FUNC_SKB_SET_TUNNEL_OPT,
            BPF_FUNC_SKB_CHANGE_PROTO,
            BPF_FUNC_SKB_CHANGE_TYPE,
            BPF_FUNC_SKB_UNDER_CGROUP,
            BPF_FUNC_GET_HASH_RECALC,
            BPF_FUNC_GET_CURRENT_TASK,
            BPF_FUNC_PROBE_WRITE_USER,
            BPF_FUNC_CURRENT_TASK_UNDER_CGROUP,
            BPF_FUNC_SKB_CHANGE_TAIL,
            BPF_FUNC_SKB_PULL_DATA,
            BPF_FUNC_CSUM_UPDATE,
        ];
        for (i, &v) in h.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_get_tunnel_pairs_adjacent() {
        // get/set tunnel key are adjacent (20/21).
        assert_eq!(BPF_FUNC_SKB_SET_TUNNEL_KEY - BPF_FUNC_SKB_GET_TUNNEL_KEY, 1);
        // get/set tunnel opt are adjacent (29/30).
        assert_eq!(BPF_FUNC_SKB_SET_TUNNEL_OPT - BPF_FUNC_SKB_GET_TUNNEL_OPT, 1);
    }

    #[test]
    fn test_csum_replace_pair() {
        // L3 / L4 checksum-replace pair.
        assert_eq!(BPF_FUNC_L4_CSUM_REPLACE - BPF_FUNC_L3_CSUM_REPLACE, 1);
    }

    #[test]
    fn test_vlan_push_pop_pair() {
        assert_eq!(BPF_FUNC_SKB_VLAN_POP - BPF_FUNC_SKB_VLAN_PUSH, 1);
    }
}
