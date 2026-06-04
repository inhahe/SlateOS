//! `<linux/bpf.h>` — `enum bpf_map_type` (BPF map kinds).
//!
//! BPF maps are typed key/value containers shared between BPF
//! programs and userspace. The numeric tag in `BPF_MAP_CREATE.type`
//! selects the data structure (hash table, array, ringbuf, …) and
//! its access semantics.

// ---------------------------------------------------------------------------
// `enum bpf_map_type` (dense from 0..32)
// ---------------------------------------------------------------------------

pub const BPF_MAP_TYPE_UNSPEC: u32 = 0;
pub const BPF_MAP_TYPE_HASH: u32 = 1;
pub const BPF_MAP_TYPE_ARRAY: u32 = 2;
pub const BPF_MAP_TYPE_PROG_ARRAY: u32 = 3;
pub const BPF_MAP_TYPE_PERF_EVENT_ARRAY: u32 = 4;
pub const BPF_MAP_TYPE_PERCPU_HASH: u32 = 5;
pub const BPF_MAP_TYPE_PERCPU_ARRAY: u32 = 6;
pub const BPF_MAP_TYPE_STACK_TRACE: u32 = 7;
pub const BPF_MAP_TYPE_CGROUP_ARRAY: u32 = 8;
pub const BPF_MAP_TYPE_LRU_HASH: u32 = 9;
pub const BPF_MAP_TYPE_LRU_PERCPU_HASH: u32 = 10;
pub const BPF_MAP_TYPE_LPM_TRIE: u32 = 11;
pub const BPF_MAP_TYPE_ARRAY_OF_MAPS: u32 = 12;
pub const BPF_MAP_TYPE_HASH_OF_MAPS: u32 = 13;
pub const BPF_MAP_TYPE_DEVMAP: u32 = 14;
pub const BPF_MAP_TYPE_SOCKMAP: u32 = 15;
pub const BPF_MAP_TYPE_CPUMAP: u32 = 16;
pub const BPF_MAP_TYPE_XSKMAP: u32 = 17;
pub const BPF_MAP_TYPE_SOCKHASH: u32 = 18;
pub const BPF_MAP_TYPE_CGROUP_STORAGE: u32 = 19;
pub const BPF_MAP_TYPE_REUSEPORT_SOCKARRAY: u32 = 20;
pub const BPF_MAP_TYPE_PERCPU_CGROUP_STORAGE: u32 = 21;
pub const BPF_MAP_TYPE_QUEUE: u32 = 22;
pub const BPF_MAP_TYPE_STACK: u32 = 23;
pub const BPF_MAP_TYPE_SK_STORAGE: u32 = 24;
pub const BPF_MAP_TYPE_DEVMAP_HASH: u32 = 25;
pub const BPF_MAP_TYPE_STRUCT_OPS: u32 = 26;
pub const BPF_MAP_TYPE_RINGBUF: u32 = 27;
pub const BPF_MAP_TYPE_INODE_STORAGE: u32 = 28;
pub const BPF_MAP_TYPE_TASK_STORAGE: u32 = 29;
pub const BPF_MAP_TYPE_BLOOM_FILTER: u32 = 30;
pub const BPF_MAP_TYPE_USER_RINGBUF: u32 = 31;
pub const BPF_MAP_TYPE_CGRP_STORAGE: u32 = 32;

pub const __MAX_BPF_MAP_TYPE: u32 = 33;

// ---------------------------------------------------------------------------
// `BPF_MAP_CREATE.map_flags` bits
// ---------------------------------------------------------------------------

pub const BPF_F_NO_PREALLOC: u32 = 1 << 0;
pub const BPF_F_NO_COMMON_LRU: u32 = 1 << 1;
pub const BPF_F_NUMA_NODE: u32 = 1 << 2;
pub const BPF_F_RDONLY: u32 = 1 << 3;
pub const BPF_F_WRONLY: u32 = 1 << 4;
pub const BPF_F_STACK_BUILD_ID: u32 = 1 << 5;
pub const BPF_F_ZERO_SEED: u32 = 1 << 6;
pub const BPF_F_RDONLY_PROG: u32 = 1 << 7;
pub const BPF_F_WRONLY_PROG: u32 = 1 << 8;
pub const BPF_F_CLONE: u32 = 1 << 9;
pub const BPF_F_MMAPABLE: u32 = 1 << 10;
pub const BPF_F_PRESERVE_ELEMS: u32 = 1 << 11;
pub const BPF_F_INNER_MAP: u32 = 1 << 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_types_dense_0_to_32() {
        // 33 enumerators 0..=32.
        assert_eq!(BPF_MAP_TYPE_UNSPEC, 0);
        assert_eq!(BPF_MAP_TYPE_CGRP_STORAGE, 32);
        assert_eq!(__MAX_BPF_MAP_TYPE, 33);
    }

    #[test]
    fn test_hash_array_pair() {
        // The two original map kinds.
        assert_eq!(BPF_MAP_TYPE_HASH, 1);
        assert_eq!(BPF_MAP_TYPE_ARRAY, 2);
    }

    #[test]
    fn test_percpu_variants_after_originals() {
        // PERCPU_HASH (5) follows PERCPU_ARRAY (6) — they cluster.
        assert!(BPF_MAP_TYPE_PERCPU_HASH < BPF_MAP_TYPE_PERCPU_ARRAY);
        assert_eq!(BPF_MAP_TYPE_PERCPU_ARRAY - BPF_MAP_TYPE_PERCPU_HASH, 1);
    }

    #[test]
    fn test_storage_family_distinct() {
        // The *_STORAGE family covers cgroup, sk, inode, task, percpu_cgroup, cgrp.
        let storage = [
            BPF_MAP_TYPE_CGROUP_STORAGE,
            BPF_MAP_TYPE_PERCPU_CGROUP_STORAGE,
            BPF_MAP_TYPE_SK_STORAGE,
            BPF_MAP_TYPE_INODE_STORAGE,
            BPF_MAP_TYPE_TASK_STORAGE,
            BPF_MAP_TYPE_CGRP_STORAGE,
        ];
        for (i, &a) in storage.iter().enumerate() {
            for &b in &storage[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn test_ringbuf_pair() {
        // RINGBUF (kernel-emits) and USER_RINGBUF (user-emits) are paired.
        assert_eq!(BPF_MAP_TYPE_RINGBUF, 27);
        assert_eq!(BPF_MAP_TYPE_USER_RINGBUF, 31);
    }

    #[test]
    fn test_map_flags_each_single_bit_distinct() {
        let f = [
            BPF_F_NO_PREALLOC,
            BPF_F_NO_COMMON_LRU,
            BPF_F_NUMA_NODE,
            BPF_F_RDONLY,
            BPF_F_WRONLY,
            BPF_F_STACK_BUILD_ID,
            BPF_F_ZERO_SEED,
            BPF_F_RDONLY_PROG,
            BPF_F_WRONLY_PROG,
            BPF_F_CLONE,
            BPF_F_MMAPABLE,
            BPF_F_PRESERVE_ELEMS,
            BPF_F_INNER_MAP,
        ];
        let mut or = 0u32;
        for (i, &v) in f.iter().enumerate() {
            assert!(v.is_power_of_two());
            assert_eq!(v, 1u32 << i);
            or |= v;
        }
        // 13 low bits.
        assert_eq!(or, 0x1FFF);
    }

    #[test]
    fn test_rdonly_wronly_pairs() {
        // RDONLY/WRONLY (UAPI side) and RDONLY_PROG/WRONLY_PROG (verifier side).
        assert_eq!(BPF_F_WRONLY, BPF_F_RDONLY << 1);
        assert_eq!(BPF_F_WRONLY_PROG, BPF_F_RDONLY_PROG << 1);
    }
}
