//! `<linux/bpf.h>` — BPF program and map type constants.
//!
//! Extended BPF (eBPF) is a programmable in-kernel virtual machine
//! used for tracing, networking, security, and observability.
//! Programs attach to hooks; maps provide shared data structures
//! between programs and userspace.

// ---------------------------------------------------------------------------
// BPF commands (for bpf() syscall)
// ---------------------------------------------------------------------------

/// Create a map.
pub const BPF_MAP_CREATE: u32 = 0;
/// Look up a map element.
pub const BPF_MAP_LOOKUP_ELEM: u32 = 1;
/// Update a map element.
pub const BPF_MAP_UPDATE_ELEM: u32 = 2;
/// Delete a map element.
pub const BPF_MAP_DELETE_ELEM: u32 = 3;
/// Iterate map elements.
pub const BPF_MAP_GET_NEXT_KEY: u32 = 4;
/// Load a BPF program.
pub const BPF_PROG_LOAD: u32 = 5;
/// Attach a BPF program.
pub const BPF_PROG_ATTACH: u32 = 8;
/// Detach a BPF program.
pub const BPF_PROG_DETACH: u32 = 9;
/// Pin an object to bpffs.
pub const BPF_OBJ_PIN: u32 = 6;
/// Get pinned object fd.
pub const BPF_OBJ_GET: u32 = 7;
/// Query attached programs.
pub const BPF_PROG_QUERY: u32 = 24;
/// Create a link.
pub const BPF_LINK_CREATE: u32 = 28;

// ---------------------------------------------------------------------------
// BPF program types
// ---------------------------------------------------------------------------

/// Unspecified program type.
pub const BPF_PROG_TYPE_UNSPEC: u32 = 0;
/// Socket filter.
pub const BPF_PROG_TYPE_SOCKET_FILTER: u32 = 1;
/// kprobe / uprobe.
pub const BPF_PROG_TYPE_KPROBE: u32 = 2;
/// Scheduler classifier.
pub const BPF_PROG_TYPE_SCHED_CLS: u32 = 3;
/// Scheduler action.
pub const BPF_PROG_TYPE_SCHED_ACT: u32 = 4;
/// Tracepoint.
pub const BPF_PROG_TYPE_TRACEPOINT: u32 = 5;
/// XDP (eXpress Data Path).
pub const BPF_PROG_TYPE_XDP: u32 = 6;
/// Perf event.
pub const BPF_PROG_TYPE_PERF_EVENT: u32 = 7;
/// Cgroup SKB.
pub const BPF_PROG_TYPE_CGROUP_SKB: u32 = 8;
/// Cgroup socket.
pub const BPF_PROG_TYPE_CGROUP_SOCK: u32 = 9;
/// Lightweight tunnel.
pub const BPF_PROG_TYPE_LWT_IN: u32 = 10;
/// Raw tracepoint.
pub const BPF_PROG_TYPE_RAW_TRACEPOINT: u32 = 17;
/// LSM hook.
pub const BPF_PROG_TYPE_LSM: u32 = 29;
/// Struct ops.
pub const BPF_PROG_TYPE_STRUCT_OPS: u32 = 30;

// ---------------------------------------------------------------------------
// BPF map types
// ---------------------------------------------------------------------------

/// Unspecified map type.
pub const BPF_MAP_TYPE_UNSPEC: u32 = 0;
/// Hash table.
pub const BPF_MAP_TYPE_HASH: u32 = 1;
/// Array.
pub const BPF_MAP_TYPE_ARRAY: u32 = 2;
/// Program array (for tail calls).
pub const BPF_MAP_TYPE_PROG_ARRAY: u32 = 3;
/// Perf event array.
pub const BPF_MAP_TYPE_PERF_EVENT_ARRAY: u32 = 4;
/// Per-CPU hash.
pub const BPF_MAP_TYPE_PERCPU_HASH: u32 = 5;
/// Per-CPU array.
pub const BPF_MAP_TYPE_PERCPU_ARRAY: u32 = 6;
/// Stack trace.
pub const BPF_MAP_TYPE_STACK_TRACE: u32 = 7;
/// LRU hash.
pub const BPF_MAP_TYPE_LRU_HASH: u32 = 9;
/// LRU per-CPU hash.
pub const BPF_MAP_TYPE_LRU_PERCPU_HASH: u32 = 10;
/// Longest prefix match trie.
pub const BPF_MAP_TYPE_LPM_TRIE: u32 = 11;
/// Ring buffer.
pub const BPF_MAP_TYPE_RINGBUF: u32 = 27;

// ---------------------------------------------------------------------------
// BPF map update flags
// ---------------------------------------------------------------------------

/// Create or update.
pub const BPF_ANY: u64 = 0;
/// Create only (fail if exists).
pub const BPF_NOEXIST: u64 = 1;
/// Update only (fail if not exists).
pub const BPF_EXIST: u64 = 2;
/// Spin-lock locked update.
pub const BPF_F_LOCK: u64 = 4;

// ---------------------------------------------------------------------------
// BPF filesystem
// ---------------------------------------------------------------------------

/// BPF filesystem type.
pub const BPFFS_TYPE: &str = "bpf";
/// Default mount point.
pub const BPFFS_MOUNT: &str = "/sys/fs/bpf";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            BPF_MAP_CREATE, BPF_MAP_LOOKUP_ELEM, BPF_MAP_UPDATE_ELEM,
            BPF_MAP_DELETE_ELEM, BPF_MAP_GET_NEXT_KEY, BPF_PROG_LOAD,
            BPF_PROG_ATTACH, BPF_PROG_DETACH, BPF_OBJ_PIN, BPF_OBJ_GET,
            BPF_PROG_QUERY, BPF_LINK_CREATE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_prog_types_distinct() {
        let types = [
            BPF_PROG_TYPE_UNSPEC, BPF_PROG_TYPE_SOCKET_FILTER,
            BPF_PROG_TYPE_KPROBE, BPF_PROG_TYPE_SCHED_CLS,
            BPF_PROG_TYPE_SCHED_ACT, BPF_PROG_TYPE_TRACEPOINT,
            BPF_PROG_TYPE_XDP, BPF_PROG_TYPE_PERF_EVENT,
            BPF_PROG_TYPE_CGROUP_SKB, BPF_PROG_TYPE_CGROUP_SOCK,
            BPF_PROG_TYPE_LWT_IN, BPF_PROG_TYPE_RAW_TRACEPOINT,
            BPF_PROG_TYPE_LSM, BPF_PROG_TYPE_STRUCT_OPS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_map_types_distinct() {
        let types = [
            BPF_MAP_TYPE_UNSPEC, BPF_MAP_TYPE_HASH,
            BPF_MAP_TYPE_ARRAY, BPF_MAP_TYPE_PROG_ARRAY,
            BPF_MAP_TYPE_PERF_EVENT_ARRAY, BPF_MAP_TYPE_PERCPU_HASH,
            BPF_MAP_TYPE_PERCPU_ARRAY, BPF_MAP_TYPE_STACK_TRACE,
            BPF_MAP_TYPE_LRU_HASH, BPF_MAP_TYPE_LRU_PERCPU_HASH,
            BPF_MAP_TYPE_LPM_TRIE, BPF_MAP_TYPE_RINGBUF,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_update_flags_distinct() {
        let flags = [BPF_ANY, BPF_NOEXIST, BPF_EXIST, BPF_F_LOCK];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_bpffs() {
        assert_eq!(BPFFS_TYPE, "bpf");
        assert!(!BPFFS_MOUNT.is_empty());
    }
}
