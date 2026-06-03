//! `<linux/bpf.h>` — Additional BPF constants.
//!
//! Supplementary BPF constants covering map types,
//! program types, attach types, and helper function IDs.

// ---------------------------------------------------------------------------
// BPF map types (BPF_MAP_TYPE_*)
// ---------------------------------------------------------------------------

/// Unspec.
pub const BPF_MAP_TYPE_UNSPEC: u32 = 0;
/// Hash.
pub const BPF_MAP_TYPE_HASH: u32 = 1;
/// Array.
pub const BPF_MAP_TYPE_ARRAY: u32 = 2;
/// Prog array.
pub const BPF_MAP_TYPE_PROG_ARRAY: u32 = 3;
/// Perf event array.
pub const BPF_MAP_TYPE_PERF_EVENT_ARRAY: u32 = 4;
/// Per-CPU hash.
pub const BPF_MAP_TYPE_PERCPU_HASH: u32 = 5;
/// Per-CPU array.
pub const BPF_MAP_TYPE_PERCPU_ARRAY: u32 = 6;
/// Stack trace.
pub const BPF_MAP_TYPE_STACK_TRACE: u32 = 7;
/// CGroup array.
pub const BPF_MAP_TYPE_CGROUP_ARRAY: u32 = 8;
/// LRU hash.
pub const BPF_MAP_TYPE_LRU_HASH: u32 = 9;
/// LRU per-CPU hash.
pub const BPF_MAP_TYPE_LRU_PERCPU_HASH: u32 = 10;
/// LPM trie.
pub const BPF_MAP_TYPE_LPM_TRIE: u32 = 11;
/// Array of maps.
pub const BPF_MAP_TYPE_ARRAY_OF_MAPS: u32 = 12;
/// Hash of maps.
pub const BPF_MAP_TYPE_HASH_OF_MAPS: u32 = 13;
/// Devmap.
pub const BPF_MAP_TYPE_DEVMAP: u32 = 14;
/// Sockmap.
pub const BPF_MAP_TYPE_SOCKMAP: u32 = 15;
/// Cpumap.
pub const BPF_MAP_TYPE_CPUMAP: u32 = 16;
/// Xskmap.
pub const BPF_MAP_TYPE_XSKMAP: u32 = 17;
/// Sockhash.
pub const BPF_MAP_TYPE_SOCKHASH: u32 = 18;
/// CGroup storage.
pub const BPF_MAP_TYPE_CGROUP_STORAGE: u32 = 19;
/// Reuseport sockarray.
pub const BPF_MAP_TYPE_REUSEPORT_SOCKARRAY: u32 = 20;
/// Per-CPU CGroup storage.
pub const BPF_MAP_TYPE_PERCPU_CGROUP_STORAGE: u32 = 21;
/// Queue.
pub const BPF_MAP_TYPE_QUEUE: u32 = 22;
/// Stack.
pub const BPF_MAP_TYPE_STACK: u32 = 23;
/// SK storage.
pub const BPF_MAP_TYPE_SK_STORAGE: u32 = 24;
/// Devmap hash.
pub const BPF_MAP_TYPE_DEVMAP_HASH: u32 = 25;
/// Struct ops.
pub const BPF_MAP_TYPE_STRUCT_OPS: u32 = 26;
/// Ringbuf.
pub const BPF_MAP_TYPE_RINGBUF: u32 = 27;
/// Inode storage.
pub const BPF_MAP_TYPE_INODE_STORAGE: u32 = 28;
/// Task storage.
pub const BPF_MAP_TYPE_TASK_STORAGE: u32 = 29;
/// Bloom filter.
pub const BPF_MAP_TYPE_BLOOM_FILTER: u32 = 30;
/// User ringbuf.
pub const BPF_MAP_TYPE_USER_RINGBUF: u32 = 31;
/// CGroup storage (v2).
pub const BPF_MAP_TYPE_CGRP_STORAGE: u32 = 32;
/// Arena.
pub const BPF_MAP_TYPE_ARENA: u32 = 33;

// ---------------------------------------------------------------------------
// BPF program types (BPF_PROG_TYPE_*)
// ---------------------------------------------------------------------------

/// Unspec.
pub const BPF_PROG_TYPE_UNSPEC: u32 = 0;
/// Socket filter.
pub const BPF_PROG_TYPE_SOCKET_FILTER: u32 = 1;
/// Kprobe.
pub const BPF_PROG_TYPE_KPROBE: u32 = 2;
/// Sched CLS.
pub const BPF_PROG_TYPE_SCHED_CLS: u32 = 3;
/// Sched ACT.
pub const BPF_PROG_TYPE_SCHED_ACT: u32 = 4;
/// Tracepoint.
pub const BPF_PROG_TYPE_TRACEPOINT: u32 = 5;
/// XDP.
pub const BPF_PROG_TYPE_XDP: u32 = 6;
/// Perf event.
pub const BPF_PROG_TYPE_PERF_EVENT: u32 = 7;
/// CGroup SKB.
pub const BPF_PROG_TYPE_CGROUP_SKB: u32 = 8;
/// CGroup sock.
pub const BPF_PROG_TYPE_CGROUP_SOCK: u32 = 9;
/// LWT in.
pub const BPF_PROG_TYPE_LWT_IN: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_types_sequential() {
        assert_eq!(BPF_MAP_TYPE_UNSPEC, 0);
        assert_eq!(BPF_MAP_TYPE_HASH, 1);
        assert_eq!(BPF_MAP_TYPE_ARENA, 33);
    }

    #[test]
    fn test_map_types_distinct() {
        let types = [
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
            BPF_MAP_TYPE_LPM_TRIE,
            BPF_MAP_TYPE_RINGBUF,
            BPF_MAP_TYPE_BLOOM_FILTER,
            BPF_MAP_TYPE_ARENA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_prog_types_sequential() {
        assert_eq!(BPF_PROG_TYPE_UNSPEC, 0);
        assert_eq!(BPF_PROG_TYPE_SOCKET_FILTER, 1);
        assert_eq!(BPF_PROG_TYPE_LWT_IN, 10);
    }
}
