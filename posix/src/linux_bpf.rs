//! `<linux/bpf.h>` — extended BPF (eBPF) interface.
//!
//! Provides constants for the `bpf()` system call: BPF commands,
//! map types, program types, and attach types.

use crate::errno;

// ---------------------------------------------------------------------------
// BPF commands (bpf() syscall first argument)
// ---------------------------------------------------------------------------

/// Create a BPF map.
pub const BPF_MAP_CREATE: u32 = 0;
/// Look up an element in a BPF map.
pub const BPF_MAP_LOOKUP_ELEM: u32 = 1;
/// Create or update an element.
pub const BPF_MAP_UPDATE_ELEM: u32 = 2;
/// Delete an element.
pub const BPF_MAP_DELETE_ELEM: u32 = 3;
/// Iterate map elements.
pub const BPF_MAP_GET_NEXT_KEY: u32 = 4;
/// Load a BPF program.
pub const BPF_PROG_LOAD: u32 = 5;
/// Attach a BPF program.
pub const BPF_PROG_ATTACH: u32 = 8;
/// Detach a BPF program.
pub const BPF_PROG_DETACH: u32 = 9;
/// Test-run a BPF program.
pub const BPF_PROG_TEST_RUN: u32 = 10;
/// Get next program ID.
pub const BPF_PROG_GET_NEXT_ID: u32 = 11;
/// Get next map ID.
pub const BPF_MAP_GET_NEXT_ID: u32 = 12;
/// Get program FD by ID.
pub const BPF_PROG_GET_FD_BY_ID: u32 = 13;
/// Get map FD by ID.
pub const BPF_MAP_GET_FD_BY_ID: u32 = 14;
/// Get object info by FD.
pub const BPF_OBJ_GET_INFO_BY_FD: u32 = 15;
/// Pin a BPF object to the filesystem.
pub const BPF_OBJ_PIN: u32 = 6;
/// Get a pinned BPF object.
pub const BPF_OBJ_GET: u32 = 7;
/// Create a BPF link.
pub const BPF_LINK_CREATE: u32 = 28;
/// Update a BPF link.
pub const BPF_LINK_UPDATE: u32 = 29;

// ---------------------------------------------------------------------------
// BPF map types
// ---------------------------------------------------------------------------

/// Hash table map.
pub const BPF_MAP_TYPE_HASH: u32 = 1;
/// Array map.
pub const BPF_MAP_TYPE_ARRAY: u32 = 2;
/// Program array (for tail calls).
pub const BPF_MAP_TYPE_PROG_ARRAY: u32 = 3;
/// Perf event array.
pub const BPF_MAP_TYPE_PERF_EVENT_ARRAY: u32 = 4;
/// Per-CPU hash table.
pub const BPF_MAP_TYPE_PERCPU_HASH: u32 = 5;
/// Per-CPU array.
pub const BPF_MAP_TYPE_PERCPU_ARRAY: u32 = 6;
/// Stack trace.
pub const BPF_MAP_TYPE_STACK_TRACE: u32 = 7;
/// LRU hash table.
pub const BPF_MAP_TYPE_LRU_HASH: u32 = 9;
/// Per-CPU LRU hash table.
pub const BPF_MAP_TYPE_LRU_PERCPU_HASH: u32 = 10;
/// Longest-prefix match trie.
pub const BPF_MAP_TYPE_LPM_TRIE: u32 = 11;
/// Array of maps.
pub const BPF_MAP_TYPE_ARRAY_OF_MAPS: u32 = 12;
/// Hash of maps.
pub const BPF_MAP_TYPE_HASH_OF_MAPS: u32 = 13;
/// Device map (XDP redirect).
pub const BPF_MAP_TYPE_DEVMAP: u32 = 14;
/// Socket map.
pub const BPF_MAP_TYPE_SOCKMAP: u32 = 15;
/// Ring buffer.
pub const BPF_MAP_TYPE_RINGBUF: u32 = 27;

// ---------------------------------------------------------------------------
// BPF program types
// ---------------------------------------------------------------------------

/// Unspecified program type.
pub const BPF_PROG_TYPE_UNSPEC: u32 = 0;
/// Socket filter.
pub const BPF_PROG_TYPE_SOCKET_FILTER: u32 = 1;
/// kprobe/uprobe.
pub const BPF_PROG_TYPE_KPROBE: u32 = 2;
/// Scheduler classifier (TC).
pub const BPF_PROG_TYPE_SCHED_CLS: u32 = 3;
/// Scheduler action (TC).
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
/// Socket operations.
pub const BPF_PROG_TYPE_SOCK_OPS: u32 = 14;
/// Socket SKB (stream parser).
pub const BPF_PROG_TYPE_SK_SKB: u32 = 15;
/// Raw tracepoint.
pub const BPF_PROG_TYPE_RAW_TRACEPOINT: u32 = 17;
/// Cgroup socket address.
pub const BPF_PROG_TYPE_CGROUP_SOCK_ADDR: u32 = 18;
/// Socket lookup.
pub const BPF_PROG_TYPE_SK_LOOKUP: u32 = 30;
/// Syscall.
pub const BPF_PROG_TYPE_SYSCALL: u32 = 31;

// ---------------------------------------------------------------------------
// XDP actions
// ---------------------------------------------------------------------------

/// Drop the packet.
pub const XDP_ABORTED: u32 = 0;
/// Drop the packet (normal drop).
pub const XDP_DROP: u32 = 1;
/// Pass to normal stack.
pub const XDP_PASS: u32 = 2;
/// Forward to another device.
pub const XDP_TX: u32 = 3;
/// Redirect.
pub const XDP_REDIRECT: u32 = 4;

// ---------------------------------------------------------------------------
// BPF update flags
// ---------------------------------------------------------------------------

/// Create new element or update existing.
pub const BPF_ANY: u64 = 0;
/// Create new element only (fail if exists).
pub const BPF_NOEXIST: u64 = 1;
/// Update existing element only (fail if doesn't exist).
pub const BPF_EXIST: u64 = 2;
/// Spin-lock locked update.
pub const BPF_F_LOCK: u64 = 4;

// ---------------------------------------------------------------------------
// bpf() syscall
// ---------------------------------------------------------------------------

/// Execute a BPF command.
///
/// Stub — returns `-1` / sets `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn bpf(_cmd: u32, _attr: *mut u8, _size: u32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

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
            BPF_OBJ_PIN, BPF_OBJ_GET, BPF_PROG_ATTACH, BPF_PROG_DETACH,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_map_types_distinct() {
        let types = [
            BPF_MAP_TYPE_HASH, BPF_MAP_TYPE_ARRAY,
            BPF_MAP_TYPE_PROG_ARRAY, BPF_MAP_TYPE_PERF_EVENT_ARRAY,
            BPF_MAP_TYPE_PERCPU_HASH, BPF_MAP_TYPE_PERCPU_ARRAY,
            BPF_MAP_TYPE_RINGBUF,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_prog_types_distinct() {
        let types = [
            BPF_PROG_TYPE_UNSPEC, BPF_PROG_TYPE_SOCKET_FILTER,
            BPF_PROG_TYPE_KPROBE, BPF_PROG_TYPE_XDP,
            BPF_PROG_TYPE_TRACEPOINT, BPF_PROG_TYPE_PERF_EVENT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_xdp_actions_sequential() {
        assert_eq!(XDP_ABORTED, 0);
        assert_eq!(XDP_DROP, 1);
        assert_eq!(XDP_PASS, 2);
        assert_eq!(XDP_TX, 3);
        assert_eq!(XDP_REDIRECT, 4);
    }

    #[test]
    fn test_update_flags() {
        assert_eq!(BPF_ANY, 0);
        assert_eq!(BPF_NOEXIST, 1);
        assert_eq!(BPF_EXIST, 2);
    }

    #[test]
    fn test_bpf_stub() {
        assert_eq!(bpf(0, core::ptr::null_mut(), 0), -1);
    }
}
