//! `<linux/bpf.h>` — bpf(2) syscall command + program/map enums.
//!
//! libbpf, bcc, and direct bpf() callers use these enums to load
//! programs, create maps, attach probes, and read attached link
//! state. This module covers the syscall command numbers and the
//! main program/map kind enums.

// ---------------------------------------------------------------------------
// bpf() commands (first arg to syscall)
// ---------------------------------------------------------------------------

/// `BPF_MAP_CREATE`.
pub const BPF_MAP_CREATE: u32 = 0;
/// `BPF_MAP_LOOKUP_ELEM`.
pub const BPF_MAP_LOOKUP_ELEM: u32 = 1;
/// `BPF_MAP_UPDATE_ELEM`.
pub const BPF_MAP_UPDATE_ELEM: u32 = 2;
/// `BPF_MAP_DELETE_ELEM`.
pub const BPF_MAP_DELETE_ELEM: u32 = 3;
/// `BPF_MAP_GET_NEXT_KEY`.
pub const BPF_MAP_GET_NEXT_KEY: u32 = 4;
/// `BPF_PROG_LOAD` — verify and load a program.
pub const BPF_PROG_LOAD: u32 = 5;
/// `BPF_OBJ_PIN` — pin an fd at /sys/fs/bpf/...
pub const BPF_OBJ_PIN: u32 = 6;
/// `BPF_OBJ_GET` — get an fd from a pinned path.
pub const BPF_OBJ_GET: u32 = 7;
/// `BPF_PROG_ATTACH`.
pub const BPF_PROG_ATTACH: u32 = 8;
/// `BPF_PROG_DETACH`.
pub const BPF_PROG_DETACH: u32 = 9;
/// `BPF_PROG_TEST_RUN`.
pub const BPF_PROG_TEST_RUN: u32 = 10;
/// `BPF_PROG_GET_NEXT_ID`.
pub const BPF_PROG_GET_NEXT_ID: u32 = 11;
/// `BPF_MAP_GET_NEXT_ID`.
pub const BPF_MAP_GET_NEXT_ID: u32 = 12;
/// `BPF_PROG_GET_FD_BY_ID`.
pub const BPF_PROG_GET_FD_BY_ID: u32 = 13;
/// `BPF_MAP_GET_FD_BY_ID`.
pub const BPF_MAP_GET_FD_BY_ID: u32 = 14;
/// `BPF_OBJ_GET_INFO_BY_FD`.
pub const BPF_OBJ_GET_INFO_BY_FD: u32 = 15;
/// `BPF_PROG_QUERY`.
pub const BPF_PROG_QUERY: u32 = 16;
/// `BPF_RAW_TRACEPOINT_OPEN`.
pub const BPF_RAW_TRACEPOINT_OPEN: u32 = 17;
/// `BPF_BTF_LOAD` — load a BTF blob.
pub const BPF_BTF_LOAD: u32 = 18;
/// `BPF_BTF_GET_FD_BY_ID`.
pub const BPF_BTF_GET_FD_BY_ID: u32 = 19;
/// `BPF_TASK_FD_QUERY`.
pub const BPF_TASK_FD_QUERY: u32 = 20;
/// `BPF_MAP_LOOKUP_AND_DELETE_ELEM`.
pub const BPF_MAP_LOOKUP_AND_DELETE_ELEM: u32 = 21;
/// `BPF_MAP_FREEZE`.
pub const BPF_MAP_FREEZE: u32 = 22;
/// `BPF_BTF_GET_NEXT_ID`.
pub const BPF_BTF_GET_NEXT_ID: u32 = 23;
/// `BPF_MAP_LOOKUP_BATCH`.
pub const BPF_MAP_LOOKUP_BATCH: u32 = 24;
/// `BPF_MAP_LOOKUP_AND_DELETE_BATCH`.
pub const BPF_MAP_LOOKUP_AND_DELETE_BATCH: u32 = 25;
/// `BPF_MAP_UPDATE_BATCH`.
pub const BPF_MAP_UPDATE_BATCH: u32 = 26;
/// `BPF_MAP_DELETE_BATCH`.
pub const BPF_MAP_DELETE_BATCH: u32 = 27;
/// `BPF_LINK_CREATE`.
pub const BPF_LINK_CREATE: u32 = 28;
/// `BPF_LINK_UPDATE`.
pub const BPF_LINK_UPDATE: u32 = 29;

// ---------------------------------------------------------------------------
// Program types (enum bpf_prog_type)
// ---------------------------------------------------------------------------

/// `BPF_PROG_TYPE_UNSPEC`.
pub const BPF_PROG_TYPE_UNSPEC: u32 = 0;
/// `BPF_PROG_TYPE_SOCKET_FILTER` — classic SO_ATTACH_FILTER.
pub const BPF_PROG_TYPE_SOCKET_FILTER: u32 = 1;
/// `BPF_PROG_TYPE_KPROBE`.
pub const BPF_PROG_TYPE_KPROBE: u32 = 2;
/// `BPF_PROG_TYPE_SCHED_CLS` — tc classifier.
pub const BPF_PROG_TYPE_SCHED_CLS: u32 = 3;
/// `BPF_PROG_TYPE_SCHED_ACT` — tc action.
pub const BPF_PROG_TYPE_SCHED_ACT: u32 = 4;
/// `BPF_PROG_TYPE_TRACEPOINT`.
pub const BPF_PROG_TYPE_TRACEPOINT: u32 = 5;
/// `BPF_PROG_TYPE_XDP`.
pub const BPF_PROG_TYPE_XDP: u32 = 6;
/// `BPF_PROG_TYPE_PERF_EVENT`.
pub const BPF_PROG_TYPE_PERF_EVENT: u32 = 7;
/// `BPF_PROG_TYPE_CGROUP_SKB`.
pub const BPF_PROG_TYPE_CGROUP_SKB: u32 = 8;
/// `BPF_PROG_TYPE_CGROUP_SOCK`.
pub const BPF_PROG_TYPE_CGROUP_SOCK: u32 = 9;
/// `BPF_PROG_TYPE_LWT_IN`.
pub const BPF_PROG_TYPE_LWT_IN: u32 = 10;
/// `BPF_PROG_TYPE_LWT_OUT`.
pub const BPF_PROG_TYPE_LWT_OUT: u32 = 11;
/// `BPF_PROG_TYPE_LWT_XMIT`.
pub const BPF_PROG_TYPE_LWT_XMIT: u32 = 12;
/// `BPF_PROG_TYPE_SOCK_OPS`.
pub const BPF_PROG_TYPE_SOCK_OPS: u32 = 13;
/// `BPF_PROG_TYPE_SK_SKB`.
pub const BPF_PROG_TYPE_SK_SKB: u32 = 14;
/// `BPF_PROG_TYPE_CGROUP_DEVICE`.
pub const BPF_PROG_TYPE_CGROUP_DEVICE: u32 = 15;

// ---------------------------------------------------------------------------
// Map types (enum bpf_map_type)
// ---------------------------------------------------------------------------

/// `BPF_MAP_TYPE_UNSPEC`.
pub const BPF_MAP_TYPE_UNSPEC: u32 = 0;
/// `BPF_MAP_TYPE_HASH`.
pub const BPF_MAP_TYPE_HASH: u32 = 1;
/// `BPF_MAP_TYPE_ARRAY`.
pub const BPF_MAP_TYPE_ARRAY: u32 = 2;
/// `BPF_MAP_TYPE_PROG_ARRAY` — tail-call table.
pub const BPF_MAP_TYPE_PROG_ARRAY: u32 = 3;
/// `BPF_MAP_TYPE_PERF_EVENT_ARRAY`.
pub const BPF_MAP_TYPE_PERF_EVENT_ARRAY: u32 = 4;
/// `BPF_MAP_TYPE_PERCPU_HASH`.
pub const BPF_MAP_TYPE_PERCPU_HASH: u32 = 5;
/// `BPF_MAP_TYPE_PERCPU_ARRAY`.
pub const BPF_MAP_TYPE_PERCPU_ARRAY: u32 = 6;
/// `BPF_MAP_TYPE_STACK_TRACE`.
pub const BPF_MAP_TYPE_STACK_TRACE: u32 = 7;
/// `BPF_MAP_TYPE_CGROUP_ARRAY`.
pub const BPF_MAP_TYPE_CGROUP_ARRAY: u32 = 8;
/// `BPF_MAP_TYPE_LRU_HASH`.
pub const BPF_MAP_TYPE_LRU_HASH: u32 = 9;
/// `BPF_MAP_TYPE_LRU_PERCPU_HASH`.
pub const BPF_MAP_TYPE_LRU_PERCPU_HASH: u32 = 10;
/// `BPF_MAP_TYPE_LPM_TRIE`.
pub const BPF_MAP_TYPE_LPM_TRIE: u32 = 11;
/// `BPF_MAP_TYPE_ARRAY_OF_MAPS`.
pub const BPF_MAP_TYPE_ARRAY_OF_MAPS: u32 = 12;
/// `BPF_MAP_TYPE_HASH_OF_MAPS`.
pub const BPF_MAP_TYPE_HASH_OF_MAPS: u32 = 13;
/// `BPF_MAP_TYPE_DEVMAP`.
pub const BPF_MAP_TYPE_DEVMAP: u32 = 14;
/// `BPF_MAP_TYPE_SOCKMAP`.
pub const BPF_MAP_TYPE_SOCKMAP: u32 = 15;
/// `BPF_MAP_TYPE_CPUMAP`.
pub const BPF_MAP_TYPE_CPUMAP: u32 = 16;
/// `BPF_MAP_TYPE_XSKMAP`.
pub const BPF_MAP_TYPE_XSKMAP: u32 = 17;
/// `BPF_MAP_TYPE_SOCKHASH`.
pub const BPF_MAP_TYPE_SOCKHASH: u32 = 18;
/// `BPF_MAP_TYPE_RINGBUF`.
pub const BPF_MAP_TYPE_RINGBUF: u32 = 27;

// ---------------------------------------------------------------------------
// BPF_PROG_LOAD flags
// ---------------------------------------------------------------------------

/// `BPF_F_STRICT_ALIGNMENT`.
pub const BPF_F_STRICT_ALIGNMENT: u32 = 1 << 0;
/// `BPF_F_ANY_ALIGNMENT`.
pub const BPF_F_ANY_ALIGNMENT: u32 = 1 << 1;
/// `BPF_F_TEST_RND_HI32`.
pub const BPF_F_TEST_RND_HI32: u32 = 1 << 2;
/// `BPF_F_TEST_STATE_FREQ`.
pub const BPF_F_TEST_STATE_FREQ: u32 = 1 << 3;
/// `BPF_F_SLEEPABLE` — program may sleep.
pub const BPF_F_SLEEPABLE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_dense() {
        let c = [
            BPF_MAP_CREATE,
            BPF_MAP_LOOKUP_ELEM,
            BPF_MAP_UPDATE_ELEM,
            BPF_MAP_DELETE_ELEM,
            BPF_MAP_GET_NEXT_KEY,
            BPF_PROG_LOAD,
            BPF_OBJ_PIN,
            BPF_OBJ_GET,
            BPF_PROG_ATTACH,
            BPF_PROG_DETACH,
            BPF_PROG_TEST_RUN,
            BPF_PROG_GET_NEXT_ID,
            BPF_MAP_GET_NEXT_ID,
            BPF_PROG_GET_FD_BY_ID,
            BPF_MAP_GET_FD_BY_ID,
            BPF_OBJ_GET_INFO_BY_FD,
            BPF_PROG_QUERY,
            BPF_RAW_TRACEPOINT_OPEN,
            BPF_BTF_LOAD,
            BPF_BTF_GET_FD_BY_ID,
            BPF_TASK_FD_QUERY,
            BPF_MAP_LOOKUP_AND_DELETE_ELEM,
            BPF_MAP_FREEZE,
            BPF_BTF_GET_NEXT_ID,
            BPF_MAP_LOOKUP_BATCH,
            BPF_MAP_LOOKUP_AND_DELETE_BATCH,
            BPF_MAP_UPDATE_BATCH,
            BPF_MAP_DELETE_BATCH,
            BPF_LINK_CREATE,
            BPF_LINK_UPDATE,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_prog_types_dense_through_15() {
        let p = [
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
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_map_types_distinct() {
        let m = [
            BPF_MAP_TYPE_UNSPEC,
            BPF_MAP_TYPE_HASH,
            BPF_MAP_TYPE_ARRAY,
            BPF_MAP_TYPE_PROG_ARRAY,
            BPF_MAP_TYPE_PERF_EVENT_ARRAY,
            BPF_MAP_TYPE_PERCPU_HASH,
            BPF_MAP_TYPE_PERCPU_ARRAY,
            BPF_MAP_TYPE_STACK_TRACE,
            BPF_MAP_TYPE_CGROUP_ARRAY,
            BPF_MAP_TYPE_LRU_HASH,
            BPF_MAP_TYPE_LRU_PERCPU_HASH,
            BPF_MAP_TYPE_LPM_TRIE,
            BPF_MAP_TYPE_ARRAY_OF_MAPS,
            BPF_MAP_TYPE_HASH_OF_MAPS,
            BPF_MAP_TYPE_DEVMAP,
            BPF_MAP_TYPE_SOCKMAP,
            BPF_MAP_TYPE_CPUMAP,
            BPF_MAP_TYPE_XSKMAP,
            BPF_MAP_TYPE_SOCKHASH,
            BPF_MAP_TYPE_RINGBUF,
        ];
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
    }

    #[test]
    fn test_load_flags_pow2_distinct() {
        let f = [
            BPF_F_STRICT_ALIGNMENT,
            BPF_F_ANY_ALIGNMENT,
            BPF_F_TEST_RND_HI32,
            BPF_F_TEST_STATE_FREQ,
            BPF_F_SLEEPABLE,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }
}
