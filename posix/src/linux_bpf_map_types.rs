//! `<linux/bpf.h>` — BPF map type constants.
//!
//! BPF maps are key-value stores shared between BPF programs and
//! userspace. These constants enumerate the different map types
//! available in the eBPF subsystem.

// ---------------------------------------------------------------------------
// BPF map types (enum bpf_map_type)
// ---------------------------------------------------------------------------

/// Unspecified map type.
pub const BPF_MAP_TYPE_UNSPEC: u32 = 0;
/// Hash table map.
pub const BPF_MAP_TYPE_HASH: u32 = 1;
/// Array map (fixed-size, integer keys).
pub const BPF_MAP_TYPE_ARRAY: u32 = 2;
/// Program array (for tail calls).
pub const BPF_MAP_TYPE_PROG_ARRAY: u32 = 3;
/// Perf event array.
pub const BPF_MAP_TYPE_PERF_EVENT_ARRAY: u32 = 4;
/// Per-CPU hash map.
pub const BPF_MAP_TYPE_PERCPU_HASH: u32 = 5;
/// Per-CPU array map.
pub const BPF_MAP_TYPE_PERCPU_ARRAY: u32 = 6;
/// Stack trace map.
pub const BPF_MAP_TYPE_STACK_TRACE: u32 = 7;
/// cgroup array.
pub const BPF_MAP_TYPE_CGROUP_ARRAY: u32 = 8;
/// LRU hash map.
pub const BPF_MAP_TYPE_LRU_HASH: u32 = 9;
/// LRU per-CPU hash map.
pub const BPF_MAP_TYPE_LRU_PERCPU_HASH: u32 = 10;
/// LPM trie (longest prefix match).
pub const BPF_MAP_TYPE_LPM_TRIE: u32 = 11;
/// Array of maps.
pub const BPF_MAP_TYPE_ARRAY_OF_MAPS: u32 = 12;
/// Hash of maps.
pub const BPF_MAP_TYPE_HASH_OF_MAPS: u32 = 13;
/// Device map (XDP redirect).
pub const BPF_MAP_TYPE_DEVMAP: u32 = 14;
/// Socket map.
pub const BPF_MAP_TYPE_SOCKMAP: u32 = 15;
/// CPU map (XDP redirect to CPU).
pub const BPF_MAP_TYPE_CPUMAP: u32 = 16;
/// XDP socket map.
pub const BPF_MAP_TYPE_XSKMAP: u32 = 17;
/// Socket hash map.
pub const BPF_MAP_TYPE_SOCKHASH: u32 = 18;
/// cgroup storage (per-cgroup).
pub const BPF_MAP_TYPE_CGROUP_STORAGE: u32 = 19;
/// Reuseport socket array.
pub const BPF_MAP_TYPE_REUSEPORT_SOCKARRAY: u32 = 20;
/// Per-CPU cgroup storage.
pub const BPF_MAP_TYPE_PERCPU_CGROUP_STORAGE: u32 = 21;
/// Queue (FIFO).
pub const BPF_MAP_TYPE_QUEUE: u32 = 22;
/// Stack (LIFO).
pub const BPF_MAP_TYPE_STACK: u32 = 23;
/// Socket-local storage.
pub const BPF_MAP_TYPE_SK_STORAGE: u32 = 24;
/// Device map with hash.
pub const BPF_MAP_TYPE_DEVMAP_HASH: u32 = 25;
/// Struct ops map.
pub const BPF_MAP_TYPE_STRUCT_OPS: u32 = 26;
/// Ring buffer map.
pub const BPF_MAP_TYPE_RINGBUF: u32 = 27;
/// Inode storage.
pub const BPF_MAP_TYPE_INODE_STORAGE: u32 = 28;
/// Task storage.
pub const BPF_MAP_TYPE_TASK_STORAGE: u32 = 29;
/// Bloom filter.
pub const BPF_MAP_TYPE_BLOOM_FILTER: u32 = 30;
/// User ring buffer.
pub const BPF_MAP_TYPE_USER_RINGBUF: u32 = 31;
/// cgroup storage (v2).
pub const BPF_MAP_TYPE_CGRP_STORAGE: u32 = 32;
/// Arena map.
pub const BPF_MAP_TYPE_ARENA: u32 = 33;

// ---------------------------------------------------------------------------
// BPF map creation flags
// ---------------------------------------------------------------------------

/// Don't prealloc map memory.
pub const BPF_F_NO_PREALLOC: u32 = 1 << 0;
/// Map is read-only for BPF programs.
pub const BPF_F_RDONLY_PROG: u32 = 1 << 3;
/// Map is write-only for BPF programs.
pub const BPF_F_WRONLY_PROG: u32 = 1 << 4;
/// Use NUMA node of the creating CPU.
pub const BPF_F_NUMA_NODE: u32 = 1 << 2;
/// Use mmap-able memory.
pub const BPF_F_MMAPABLE: u32 = 1 << 10;
/// Inner map shares outer map's memory.
pub const BPF_F_INNER_MAP: u32 = 1 << 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
            BPF_MAP_TYPE_LRU_PERCPU_HASH,
            BPF_MAP_TYPE_LPM_TRIE,
            BPF_MAP_TYPE_ARRAY_OF_MAPS,
            BPF_MAP_TYPE_HASH_OF_MAPS,
            BPF_MAP_TYPE_DEVMAP,
            BPF_MAP_TYPE_SOCKMAP,
            BPF_MAP_TYPE_CPUMAP,
            BPF_MAP_TYPE_XSKMAP,
            BPF_MAP_TYPE_SOCKHASH,
            BPF_MAP_TYPE_CGROUP_STORAGE,
            BPF_MAP_TYPE_REUSEPORT_SOCKARRAY,
            BPF_MAP_TYPE_PERCPU_CGROUP_STORAGE,
            BPF_MAP_TYPE_QUEUE,
            BPF_MAP_TYPE_STACK,
            BPF_MAP_TYPE_SK_STORAGE,
            BPF_MAP_TYPE_DEVMAP_HASH,
            BPF_MAP_TYPE_STRUCT_OPS,
            BPF_MAP_TYPE_RINGBUF,
            BPF_MAP_TYPE_INODE_STORAGE,
            BPF_MAP_TYPE_TASK_STORAGE,
            BPF_MAP_TYPE_BLOOM_FILTER,
            BPF_MAP_TYPE_USER_RINGBUF,
            BPF_MAP_TYPE_CGRP_STORAGE,
            BPF_MAP_TYPE_ARENA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(BPF_MAP_TYPE_UNSPEC, 0);
    }

    #[test]
    fn test_creation_flags_no_overlap() {
        let flags = [
            BPF_F_NO_PREALLOC,
            BPF_F_RDONLY_PROG,
            BPF_F_WRONLY_PROG,
            BPF_F_NUMA_NODE,
            BPF_F_MMAPABLE,
            BPF_F_INNER_MAP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ringbuf_type() {
        assert_eq!(BPF_MAP_TYPE_RINGBUF, 27);
    }
}
