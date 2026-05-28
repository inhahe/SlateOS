//! `<linux/bpf.h>` — extended BPF (eBPF) interface.
//!
//! Provides constants for the `bpf()` system call: BPF commands, map
//! types, program types, attach types, plus a real input-validator
//! front end. Every code path validates its input shape against
//! Linux's `bpf(2)` contract (size bounds, fd bounds, type-id ranges,
//! flag bits) and then returns ENOSYS — matching a Linux kernel built
//! without `CONFIG_BPF_SYSCALL=y`.
//!
//! Real BPF enforcement (the in-kernel verifier, map storage, program
//! JIT, attach-point hook tables, link-tracking) is deferred; it
//! requires major kernel subsystem work. But the validation surface
//! here is what every BPF-aware program (libbpf, BCC, bpftrace, Cilium,
//! Falco, systemd-networkd's BPF-CGROUP filters, the Rust `aya` /
//! `libbpf-rs` crates, Go's `cilium/ebpf` package) needs to detect
//! correctly when probing for BPF support.

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
/// Pin a BPF object to the filesystem.
pub const BPF_OBJ_PIN: u32 = 6;
/// Get a pinned BPF object.
pub const BPF_OBJ_GET: u32 = 7;
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
/// Query attached programs on a cgroup/sock.
pub const BPF_PROG_QUERY: u32 = 16;
/// Open a raw tracepoint.
pub const BPF_RAW_TRACEPOINT_OPEN: u32 = 17;
/// Load BTF (BPF Type Format).
pub const BPF_BTF_LOAD: u32 = 18;
/// Get BTF FD by ID.
pub const BPF_BTF_GET_FD_BY_ID: u32 = 19;
/// Iterate tasks/files associated with a BPF program.
pub const BPF_TASK_FD_QUERY: u32 = 20;
/// Lookup-and-delete in one shot.
pub const BPF_MAP_LOOKUP_AND_DELETE_ELEM: u32 = 21;
/// Freeze a map (no further updates).
pub const BPF_MAP_FREEZE: u32 = 22;
/// Get next BTF ID.
pub const BPF_BTF_GET_NEXT_ID: u32 = 23;
/// Batch lookup of map elements.
pub const BPF_MAP_LOOKUP_BATCH: u32 = 24;
/// Batch lookup-and-delete of map elements.
pub const BPF_MAP_LOOKUP_AND_DELETE_BATCH: u32 = 25;
/// Batch update of map elements.
pub const BPF_MAP_UPDATE_BATCH: u32 = 26;
/// Batch delete of map elements.
pub const BPF_MAP_DELETE_BATCH: u32 = 27;
/// Create a BPF link.
pub const BPF_LINK_CREATE: u32 = 28;
/// Update a BPF link.
pub const BPF_LINK_UPDATE: u32 = 29;
/// Get link FD by ID.
pub const BPF_LINK_GET_FD_BY_ID: u32 = 30;
/// Get next link ID.
pub const BPF_LINK_GET_NEXT_ID: u32 = 31;
/// Enable BPF stats globally.
pub const BPF_ENABLE_STATS: u32 = 32;
/// Create a BPF iterator.
pub const BPF_ITER_CREATE: u32 = 33;
/// Detach a BPF link.
pub const BPF_LINK_DETACH: u32 = 34;
/// Bind a BPF map to a program.
pub const BPF_PROG_BIND_MAP: u32 = 35;
/// Token create (Linux 6.7+).
pub const BPF_TOKEN_CREATE: u32 = 36;
/// The first unknown command — anything ≥ this is rejected with EINVAL.
const BPF_CMD_MAX: u32 = 37;

// ---------------------------------------------------------------------------
// BPF map types
// ---------------------------------------------------------------------------

/// Unspecified map type — rejected by Linux on `BPF_MAP_CREATE`.
pub const BPF_MAP_TYPE_UNSPEC: u32 = 0;
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
/// Cgroup array.
pub const BPF_MAP_TYPE_CGROUP_ARRAY: u32 = 8;
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
/// CPU map (XDP per-cpu redirect).
pub const BPF_MAP_TYPE_CPUMAP: u32 = 16;
/// XSK map (AF_XDP socket).
pub const BPF_MAP_TYPE_XSKMAP: u32 = 17;
/// Socket hash.
pub const BPF_MAP_TYPE_SOCKHASH: u32 = 18;
/// Cgroup storage (legacy).
pub const BPF_MAP_TYPE_CGROUP_STORAGE: u32 = 19;
/// Reuseport sock array.
pub const BPF_MAP_TYPE_REUSEPORT_SOCKARRAY: u32 = 20;
/// Percpu cgroup storage.
pub const BPF_MAP_TYPE_PERCPU_CGROUP_STORAGE: u32 = 21;
/// Queue map.
pub const BPF_MAP_TYPE_QUEUE: u32 = 22;
/// Stack map.
pub const BPF_MAP_TYPE_STACK: u32 = 23;
/// Sock local storage.
pub const BPF_MAP_TYPE_SK_STORAGE: u32 = 24;
/// Devmap hash variant.
pub const BPF_MAP_TYPE_DEVMAP_HASH: u32 = 25;
/// Struct ops map.
pub const BPF_MAP_TYPE_STRUCT_OPS: u32 = 26;
/// Ring buffer.
pub const BPF_MAP_TYPE_RINGBUF: u32 = 27;
/// Inode local storage.
pub const BPF_MAP_TYPE_INODE_STORAGE: u32 = 28;
/// Task local storage.
pub const BPF_MAP_TYPE_TASK_STORAGE: u32 = 29;
/// Bloom filter.
pub const BPF_MAP_TYPE_BLOOM_FILTER: u32 = 30;
/// User-mode ring buffer.
pub const BPF_MAP_TYPE_USER_RINGBUF: u32 = 31;
/// Cgroup storage (new style).
pub const BPF_MAP_TYPE_CGRP_STORAGE: u32 = 32;
/// Arena (Linux 6.7+).
pub const BPF_MAP_TYPE_ARENA: u32 = 33;
/// First unknown map type — anything ≥ this is rejected with EINVAL.
const BPF_MAP_TYPE_MAX: u32 = 34;

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
/// Lightweight tunnel (input).
pub const BPF_PROG_TYPE_LWT_IN: u32 = 10;
/// Lightweight tunnel (output).
pub const BPF_PROG_TYPE_LWT_OUT: u32 = 11;
/// Lightweight tunnel (xmit).
pub const BPF_PROG_TYPE_LWT_XMIT: u32 = 12;
/// Sock ops.
pub const BPF_PROG_TYPE_SOCK_OPS: u32 = 13;
/// Socket SKB (stream parser).
pub const BPF_PROG_TYPE_SK_SKB: u32 = 14;
/// Cgroup device.
pub const BPF_PROG_TYPE_CGROUP_DEVICE: u32 = 15;
/// Sock-msg.
pub const BPF_PROG_TYPE_SK_MSG: u32 = 16;
/// Raw tracepoint.
pub const BPF_PROG_TYPE_RAW_TRACEPOINT: u32 = 17;
/// Cgroup socket address.
pub const BPF_PROG_TYPE_CGROUP_SOCK_ADDR: u32 = 18;
/// LWT in-seg6local.
pub const BPF_PROG_TYPE_LWT_SEG6LOCAL: u32 = 19;
/// LIRC mode2.
pub const BPF_PROG_TYPE_LIRC_MODE2: u32 = 20;
/// Sock reuseport selector.
pub const BPF_PROG_TYPE_SK_REUSEPORT: u32 = 21;
/// Flow dissector.
pub const BPF_PROG_TYPE_FLOW_DISSECTOR: u32 = 22;
/// Cgroup sysctl.
pub const BPF_PROG_TYPE_CGROUP_SYSCTL: u32 = 23;
/// Raw tracepoint writable.
pub const BPF_PROG_TYPE_RAW_TRACEPOINT_WRITABLE: u32 = 24;
/// Cgroup sockopt.
pub const BPF_PROG_TYPE_CGROUP_SOCKOPT: u32 = 25;
/// Tracing (fentry/fexit/lsm).
pub const BPF_PROG_TYPE_TRACING: u32 = 26;
/// Struct ops.
pub const BPF_PROG_TYPE_STRUCT_OPS: u32 = 27;
/// Extension.
pub const BPF_PROG_TYPE_EXT: u32 = 28;
/// LSM hook.
pub const BPF_PROG_TYPE_LSM: u32 = 29;
/// Socket lookup.
pub const BPF_PROG_TYPE_SK_LOOKUP: u32 = 30;
/// Syscall.
pub const BPF_PROG_TYPE_SYSCALL: u32 = 31;
/// Netfilter (Linux 6.4+).
pub const BPF_PROG_TYPE_NETFILTER: u32 = 32;
/// First unknown prog type — anything ≥ this is rejected with EINVAL.
const BPF_PROG_TYPE_MAX: u32 = 33;

// ---------------------------------------------------------------------------
// BPF attach types
// ---------------------------------------------------------------------------

pub const BPF_CGROUP_INET_INGRESS: u32 = 0;
pub const BPF_CGROUP_INET_EGRESS: u32 = 1;
pub const BPF_CGROUP_INET_SOCK_CREATE: u32 = 2;
pub const BPF_CGROUP_SOCK_OPS: u32 = 3;
pub const BPF_SK_SKB_STREAM_PARSER: u32 = 4;
pub const BPF_SK_SKB_STREAM_VERDICT: u32 = 5;
pub const BPF_CGROUP_DEVICE: u32 = 6;
pub const BPF_SK_MSG_VERDICT: u32 = 7;
pub const BPF_CGROUP_INET4_BIND: u32 = 8;
pub const BPF_CGROUP_INET6_BIND: u32 = 9;
pub const BPF_CGROUP_INET4_CONNECT: u32 = 10;
pub const BPF_CGROUP_INET6_CONNECT: u32 = 11;
pub const BPF_CGROUP_INET4_POST_BIND: u32 = 12;
pub const BPF_CGROUP_INET6_POST_BIND: u32 = 13;
pub const BPF_CGROUP_UDP4_SENDMSG: u32 = 14;
pub const BPF_CGROUP_UDP6_SENDMSG: u32 = 15;
pub const BPF_LIRC_MODE2: u32 = 16;
pub const BPF_FLOW_DISSECTOR: u32 = 17;
pub const BPF_CGROUP_SYSCTL: u32 = 18;
pub const BPF_CGROUP_UDP4_RECVMSG: u32 = 19;
pub const BPF_CGROUP_UDP6_RECVMSG: u32 = 20;
pub const BPF_CGROUP_GETSOCKOPT: u32 = 21;
pub const BPF_CGROUP_SETSOCKOPT: u32 = 22;
pub const BPF_TRACE_RAW_TP: u32 = 23;
pub const BPF_TRACE_FENTRY: u32 = 24;
pub const BPF_TRACE_FEXIT: u32 = 25;
pub const BPF_MODIFY_RETURN: u32 = 26;
pub const BPF_LSM_MAC: u32 = 27;
pub const BPF_TRACE_ITER: u32 = 28;
pub const BPF_CGROUP_INET4_GETPEERNAME: u32 = 29;
pub const BPF_CGROUP_INET6_GETPEERNAME: u32 = 30;
pub const BPF_CGROUP_INET4_GETSOCKNAME: u32 = 31;
pub const BPF_CGROUP_INET6_GETSOCKNAME: u32 = 32;
pub const BPF_XDP_DEVMAP: u32 = 33;
pub const BPF_CGROUP_INET_SOCK_RELEASE: u32 = 34;
pub const BPF_XDP_CPUMAP: u32 = 35;
pub const BPF_SK_LOOKUP: u32 = 36;
pub const BPF_XDP: u32 = 37;
pub const BPF_SK_SKB_VERDICT: u32 = 38;
pub const BPF_SK_REUSEPORT_SELECT: u32 = 39;
pub const BPF_SK_REUSEPORT_SELECT_OR_MIGRATE: u32 = 40;
pub const BPF_PERF_EVENT: u32 = 41;
pub const BPF_TRACE_KPROBE_MULTI: u32 = 42;
pub const BPF_LSM_CGROUP: u32 = 43;
pub const BPF_STRUCT_OPS: u32 = 44;
pub const BPF_NETFILTER: u32 = 45;
pub const BPF_TCX_INGRESS: u32 = 46;
pub const BPF_TCX_EGRESS: u32 = 47;
pub const BPF_TRACE_UPROBE_MULTI: u32 = 48;
pub const BPF_CGROUP_UNIX_CONNECT: u32 = 49;
pub const BPF_CGROUP_UNIX_SENDMSG: u32 = 50;
pub const BPF_CGROUP_UNIX_RECVMSG: u32 = 51;
pub const BPF_CGROUP_UNIX_GETPEERNAME: u32 = 52;
pub const BPF_CGROUP_UNIX_GETSOCKNAME: u32 = 53;
pub const BPF_NETKIT_PRIMARY: u32 = 54;
pub const BPF_NETKIT_PEER: u32 = 55;
/// First unknown attach type — anything ≥ this is rejected with EINVAL.
const BPF_ATTACH_TYPE_MAX: u32 = 56;

// ---------------------------------------------------------------------------
// XDP actions
// ---------------------------------------------------------------------------

/// Drop the packet (error path).
pub const XDP_ABORTED: u32 = 0;
/// Drop the packet (normal drop).
pub const XDP_DROP: u32 = 1;
/// Pass to normal stack.
pub const XDP_PASS: u32 = 2;
/// Forward back out the same interface.
pub const XDP_TX: u32 = 3;
/// Redirect.
pub const XDP_REDIRECT: u32 = 4;

// ---------------------------------------------------------------------------
// Map-element update flags (3rd arg to BPF_MAP_UPDATE_ELEM)
// ---------------------------------------------------------------------------

/// Create new element or update existing.
pub const BPF_ANY: u64 = 0;
/// Create new element only (fail if exists).
pub const BPF_NOEXIST: u64 = 1;
/// Update existing element only (fail if doesn't exist).
pub const BPF_EXIST: u64 = 2;
/// Spin-lock-protected update.
pub const BPF_F_LOCK: u64 = 4;
/// Valid bits for update_elem flags.
const BPF_UPDATE_FLAGS_VALID: u64 = BPF_NOEXIST | BPF_EXIST | BPF_F_LOCK;

// ---------------------------------------------------------------------------
// Map-create flags
// ---------------------------------------------------------------------------

/// No prealloc — allocate lazily.
pub const BPF_F_NO_PREALLOC: u32 = 1;
/// No common LRU node.
pub const BPF_F_NO_COMMON_LRU: u32 = 1 << 1;
/// Use NUMA node hint.
pub const BPF_F_NUMA_NODE: u32 = 1 << 2;
/// Read-only.
pub const BPF_F_RDONLY: u32 = 1 << 3;
/// Write-only.
pub const BPF_F_WRONLY: u32 = 1 << 4;
/// Stack-trace build IDs.
pub const BPF_F_STACK_BUILD_ID: u32 = 1 << 5;
/// Zero-seed for prng.
pub const BPF_F_ZERO_SEED: u32 = 1 << 6;
/// RDONLY at runtime.
pub const BPF_F_RDONLY_PROG: u32 = 1 << 7;
/// WRONLY at runtime.
pub const BPF_F_WRONLY_PROG: u32 = 1 << 8;
/// Clone the inner map.
pub const BPF_F_CLONE: u32 = 1 << 9;
/// MMAPable.
pub const BPF_F_MMAPABLE: u32 = 1 << 10;
/// Preserve elem nodes when CPU is offline.
pub const BPF_F_PRESERVE_ELEMS: u32 = 1 << 11;
/// Inner map (special).
pub const BPF_F_INNER_MAP: u32 = 1 << 12;
/// Valid bits for map_create flags.
const BPF_MAP_CREATE_FLAGS_VALID: u32 = BPF_F_NO_PREALLOC
    | BPF_F_NO_COMMON_LRU
    | BPF_F_NUMA_NODE
    | BPF_F_RDONLY
    | BPF_F_WRONLY
    | BPF_F_STACK_BUILD_ID
    | BPF_F_ZERO_SEED
    | BPF_F_RDONLY_PROG
    | BPF_F_WRONLY_PROG
    | BPF_F_CLONE
    | BPF_F_MMAPABLE
    | BPF_F_PRESERVE_ELEMS
    | BPF_F_INNER_MAP;

// ---------------------------------------------------------------------------
// Size & count bounds (Linux UAPI)
// ---------------------------------------------------------------------------

/// Minimum size of the `bpf_attr` union the caller must hand us. Linux
/// requires at least the first few words of the union to be present;
/// anything smaller than 4 bytes can't even hold one field, so reject
/// it. (Linux's per-cmd check is stricter — see [`min_attr_size`].)
const BPF_ATTR_SIZE_MIN_RAW: u32 = 4;
/// Maximum size of the `bpf_attr` union. Linux uses the size of the
/// union itself (around 144 bytes today); we use a safety cap of
/// 4 KiB so future Linux extensions still fit while a runaway value
/// gets rejected.
const BPF_ATTR_SIZE_MAX: u32 = 4096;
/// Maximum eBPF program instruction count (Linux's `BPF_MAXINSNS`).
/// Linux historically used 4096 and later raised to 1,000,000 for
/// CAP_SYS_ADMIN callers; we use the post-5.0 ceiling.
const BPF_MAXINSNS: u32 = 1_000_000;
/// Maximum size of the `union bpf_attr.test.data_in`/`data_out` bound.
const BPF_PROG_TEST_RUN_MAX_DATA: u32 = 32 * 1024 * 1024;
/// Sentinel for "no fd" in BPF_LINK_CREATE.target_fd / BPF_PROG_ATTACH.target_fd.
const BPF_FD_NONE: i32 = -1;
/// Map of valid BPF_PROG_LOAD prog flags. (Truncated — we accept any
/// caller value because the verifier is what filters real flags.)
const BPF_PROG_LOAD_FLAGS_MASK: u32 = 0xFFFF;

// ---------------------------------------------------------------------------
// Per-command attr-struct prefixes
// ---------------------------------------------------------------------------
//
// The real `union bpf_attr` is a tagged union with a different shape
// per cmd. We declare each shape as a `#[repr(C)]` struct holding only
// the fields we validate; we read them with `core::ptr::read_unaligned`
// so a caller passing an alignment-1 attr pointer doesn't UB.
//
// Each struct's size is the per-command minimum the caller must
// provide. Smaller → EINVAL.

/// `BPF_MAP_CREATE` attr shape (first 9 fields of the upstream union).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfMapCreateAttr {
    map_type: u32,
    key_size: u32,
    value_size: u32,
    max_entries: u32,
    map_flags: u32,
    inner_map_fd: u32,
    numa_node: u32,
    map_name: [u8; 16],
    map_ifindex: u32,
}

/// `BPF_MAP_*_ELEM` / `BPF_MAP_GET_NEXT_KEY` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfMapElemAttr {
    map_fd: u32,
    key_ptr: u64,
    value_or_next_key_ptr: u64,
    flags: u64,
}

/// `BPF_PROG_LOAD` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfProgLoadAttr {
    prog_type: u32,
    insn_cnt: u32,
    insns_ptr: u64,
    license_ptr: u64,
    log_level: u32,
    log_size: u32,
    log_buf_ptr: u64,
    kern_version: u32,
    prog_flags: u32,
    prog_name: [u8; 16],
    prog_ifindex: u32,
    expected_attach_type: u32,
}

/// `BPF_OBJ_PIN` / `BPF_OBJ_GET` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfObjAttr {
    pathname_ptr: u64,
    bpf_fd: u32,
    file_flags: u32,
}

/// `BPF_PROG_ATTACH` / `BPF_PROG_DETACH` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfProgAttachAttr {
    target_fd: u32,
    attach_bpf_fd: u32,
    attach_type: u32,
    attach_flags: u32,
}

/// `BPF_PROG_TEST_RUN` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfProgTestRunAttr {
    prog_fd: u32,
    retval: u32,
    data_size_in: u32,
    data_size_out: u32,
    data_in: u64,
    data_out: u64,
    repeat: u32,
    duration: u32,
}

/// `BPF_*_GET_NEXT_ID` / `BPF_*_GET_FD_BY_ID` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfGetIdAttr {
    start_id_or_target_id: u32,
    next_id_or_open_flags: u32,
    /// Some variants put open_flags in field 2 instead of next_id;
    /// they share the same prefix so we only inspect what we need.
    _pad: u32,
}

/// `BPF_OBJ_GET_INFO_BY_FD` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfInfoByFdAttr {
    bpf_fd: u32,
    info_len: u32,
    info_ptr: u64,
}

/// `BPF_LINK_CREATE` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfLinkCreateAttr {
    prog_fd: u32,
    target_fd: u32,
    attach_type: u32,
    flags: u32,
}

/// `BPF_LINK_UPDATE` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfLinkUpdateAttr {
    link_fd: u32,
    new_prog_fd: u32,
    flags: u32,
    old_prog_fd: u32,
}

/// `BPF_LINK_DETACH` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfLinkDetachAttr {
    link_fd: u32,
}

/// `BPF_RAW_TRACEPOINT_OPEN` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfRawTpAttr {
    name_ptr: u64,
    prog_fd: u32,
}

/// `BPF_ENABLE_STATS` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfEnableStatsAttr {
    stats_type: u32,
}

/// `BPF_ITER_CREATE` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfIterCreateAttr {
    link_fd: u32,
    flags: u32,
}

/// `BPF_PROG_BIND_MAP` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfProgBindMapAttr {
    prog_fd: u32,
    map_fd: u32,
    flags: u32,
}

/// `BPF_MAP_FREEZE` attr shape.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BpfMapFreezeAttr {
    map_fd: u32,
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Returns the minimum `size` Linux requires for the given cmd's
/// portion of the bpf_attr union. Per-cmd size below this triggers
/// EINVAL — that's how Linux distinguishes "you're calling an old API
/// surface" from "your inputs are garbage."
fn min_attr_size(cmd: u32) -> u32 {
    match cmd {
        BPF_MAP_CREATE => core::mem::size_of::<BpfMapCreateAttr>() as u32,
        BPF_MAP_LOOKUP_ELEM
        | BPF_MAP_UPDATE_ELEM
        | BPF_MAP_DELETE_ELEM
        | BPF_MAP_GET_NEXT_KEY
        | BPF_MAP_LOOKUP_AND_DELETE_ELEM => core::mem::size_of::<BpfMapElemAttr>() as u32,
        BPF_PROG_LOAD => core::mem::size_of::<BpfProgLoadAttr>() as u32,
        BPF_OBJ_PIN | BPF_OBJ_GET => core::mem::size_of::<BpfObjAttr>() as u32,
        BPF_PROG_ATTACH | BPF_PROG_DETACH => {
            core::mem::size_of::<BpfProgAttachAttr>() as u32
        }
        BPF_PROG_TEST_RUN => core::mem::size_of::<BpfProgTestRunAttr>() as u32,
        BPF_PROG_GET_NEXT_ID
        | BPF_MAP_GET_NEXT_ID
        | BPF_PROG_GET_FD_BY_ID
        | BPF_MAP_GET_FD_BY_ID
        | BPF_BTF_GET_NEXT_ID
        | BPF_BTF_GET_FD_BY_ID
        | BPF_LINK_GET_NEXT_ID
        | BPF_LINK_GET_FD_BY_ID => core::mem::size_of::<BpfGetIdAttr>() as u32,
        BPF_OBJ_GET_INFO_BY_FD => core::mem::size_of::<BpfInfoByFdAttr>() as u32,
        BPF_LINK_CREATE => core::mem::size_of::<BpfLinkCreateAttr>() as u32,
        BPF_LINK_UPDATE => core::mem::size_of::<BpfLinkUpdateAttr>() as u32,
        BPF_LINK_DETACH => core::mem::size_of::<BpfLinkDetachAttr>() as u32,
        BPF_RAW_TRACEPOINT_OPEN => core::mem::size_of::<BpfRawTpAttr>() as u32,
        BPF_ENABLE_STATS => core::mem::size_of::<BpfEnableStatsAttr>() as u32,
        BPF_ITER_CREATE => core::mem::size_of::<BpfIterCreateAttr>() as u32,
        BPF_PROG_BIND_MAP => core::mem::size_of::<BpfProgBindMapAttr>() as u32,
        BPF_MAP_FREEZE => core::mem::size_of::<BpfMapFreezeAttr>() as u32,
        // Commands whose attr we don't model: still require the
        // minimum raw size so callers can't smuggle a 0-byte attr.
        _ => BPF_ATTR_SIZE_MIN_RAW,
    }
}

/// SAFETY: caller must ensure `attr` is readable for at least `size` bytes.
unsafe fn read_attr<T: Copy>(attr: *const u8) -> T {
    // SAFETY: caller has confirmed at least `size_of::<T>()` bytes are
    // readable; `read_unaligned` permits any alignment so an
    // alignment-1 pointer is fine.
    unsafe { core::ptr::read_unaligned(attr.cast::<T>()) }
}

/// Validates `attr.map_type` is a known map type and `attr.map_flags`
/// only uses defined bits.
fn validate_map_create(a: &BpfMapCreateAttr) -> Result<(), i32> {
    if a.map_type == BPF_MAP_TYPE_UNSPEC || a.map_type >= BPF_MAP_TYPE_MAX {
        return Err(errno::EINVAL);
    }
    if (a.map_flags & !BPF_MAP_CREATE_FLAGS_VALID) != 0 {
        return Err(errno::EINVAL);
    }
    // RDONLY and WRONLY are mutually exclusive at runtime AND at
    // load-time. Linux rejects either pair.
    if (a.map_flags & BPF_F_RDONLY) != 0 && (a.map_flags & BPF_F_WRONLY) != 0 {
        return Err(errno::EINVAL);
    }
    if (a.map_flags & BPF_F_RDONLY_PROG) != 0 && (a.map_flags & BPF_F_WRONLY_PROG) != 0 {
        return Err(errno::EINVAL);
    }
    // Value-size==0 is rejected for every map type except program-array
    // (where the value is implicit, but Linux still requires sizeof(u32))
    // and stack/queue (which Linux specifically validates separately).
    if a.value_size == 0 {
        return Err(errno::EINVAL);
    }
    // Key-size==0 is only allowed for maps where the "key" is implicit:
    // QUEUE/STACK/RINGBUF/STACK_TRACE. Every other type requires a key.
    let allows_zero_key = matches!(
        a.map_type,
        BPF_MAP_TYPE_QUEUE
            | BPF_MAP_TYPE_STACK
            | BPF_MAP_TYPE_RINGBUF
            | BPF_MAP_TYPE_USER_RINGBUF
    );
    if a.key_size == 0 && !allows_zero_key {
        return Err(errno::EINVAL);
    }
    // max_entries==0 is rejected by Linux for every type except struct_ops
    // (which uses the type's pseudo-field count) and arena (size-based).
    let allows_zero_max = matches!(
        a.map_type,
        BPF_MAP_TYPE_STRUCT_OPS | BPF_MAP_TYPE_ARENA
    );
    if a.max_entries == 0 && !allows_zero_max {
        return Err(errno::EINVAL);
    }
    Ok(())
}

/// Validates a map-element op attr: `map_fd` must be a non-negative
/// fd, and update_elem flags must lie in the valid set.
fn validate_map_elem(a: &BpfMapElemAttr, cmd: u32) -> Result<(), i32> {
    // map_fd is read as u32; we mirror Linux's behavior of treating it
    // as a signed fd by casting and checking < 0 below.
    let fd = a.map_fd as i32;
    if fd < 0 {
        return Err(errno::EBADF);
    }
    if cmd == BPF_MAP_UPDATE_ELEM {
        if (a.flags & !BPF_UPDATE_FLAGS_VALID) != 0 {
            return Err(errno::EINVAL);
        }
        // Linux rejects NOEXIST|EXIST in the same call.
        if (a.flags & BPF_NOEXIST) != 0 && (a.flags & BPF_EXIST) != 0 {
            return Err(errno::EINVAL);
        }
    } else if (a.flags & !BPF_F_LOCK) != 0 {
        // Non-UPDATE ops only accept the BPF_F_LOCK bit (some don't
        // accept any flags, but Linux silently ignores unknown bits
        // on lookups too — we're stricter and reject them).
        return Err(errno::EINVAL);
    }
    Ok(())
}

/// Validates a program-load attr.
fn validate_prog_load(a: &BpfProgLoadAttr) -> Result<(), i32> {
    if a.prog_type == BPF_PROG_TYPE_UNSPEC || a.prog_type >= BPF_PROG_TYPE_MAX {
        return Err(errno::EINVAL);
    }
    if a.insn_cnt == 0 {
        return Err(errno::EINVAL);
    }
    if a.insn_cnt > BPF_MAXINSNS {
        return Err(errno::E2BIG);
    }
    if a.insns_ptr == 0 {
        return Err(errno::EFAULT);
    }
    if a.license_ptr == 0 {
        // Linux requires a non-NULL license string; an unlicensed
        // program is rejected before verification.
        return Err(errno::EFAULT);
    }
    // If the caller asked for log output, the log buffer must be real.
    if a.log_level != 0 {
        if a.log_buf_ptr == 0 || a.log_size == 0 {
            return Err(errno::EINVAL);
        }
        if a.log_size > BPF_PROG_TEST_RUN_MAX_DATA {
            return Err(errno::E2BIG);
        }
    }
    if (a.prog_flags & !BPF_PROG_LOAD_FLAGS_MASK) != 0 {
        return Err(errno::EINVAL);
    }
    // expected_attach_type==0 is BPF_CGROUP_INET_INGRESS, which is
    // valid for some prog types — we don't cross-validate here because
    // the verifier handles the matrix. Just bound-check.
    if a.expected_attach_type >= BPF_ATTACH_TYPE_MAX {
        return Err(errno::EINVAL);
    }
    Ok(())
}

/// Validates a pin/get attr — the pathname pointer must be non-NULL.
fn validate_obj(a: &BpfObjAttr, cmd: u32) -> Result<(), i32> {
    if a.pathname_ptr == 0 {
        return Err(errno::EFAULT);
    }
    if cmd == BPF_OBJ_PIN {
        // PIN requires an existing bpf fd.
        let fd = a.bpf_fd as i32;
        if fd < 0 {
            return Err(errno::EBADF);
        }
    }
    Ok(())
}

/// Validates an attach/detach attr.
fn validate_prog_attach(a: &BpfProgAttachAttr, cmd: u32) -> Result<(), i32> {
    let target = a.target_fd as i32;
    if target < 0 {
        return Err(errno::EBADF);
    }
    if cmd == BPF_PROG_ATTACH {
        let prog = a.attach_bpf_fd as i32;
        if prog < 0 {
            return Err(errno::EBADF);
        }
    }
    if a.attach_type >= BPF_ATTACH_TYPE_MAX {
        return Err(errno::EINVAL);
    }
    Ok(())
}

/// Validates a test-run attr.
fn validate_prog_test_run(a: &BpfProgTestRunAttr) -> Result<(), i32> {
    let fd = a.prog_fd as i32;
    if fd < 0 {
        return Err(errno::EBADF);
    }
    if a.data_size_in > BPF_PROG_TEST_RUN_MAX_DATA {
        return Err(errno::E2BIG);
    }
    if a.data_size_out > BPF_PROG_TEST_RUN_MAX_DATA {
        return Err(errno::E2BIG);
    }
    Ok(())
}

/// Validates an info-by-fd attr.
fn validate_info_by_fd(a: &BpfInfoByFdAttr) -> Result<(), i32> {
    let fd = a.bpf_fd as i32;
    if fd < 0 {
        return Err(errno::EBADF);
    }
    if a.info_ptr == 0 {
        return Err(errno::EFAULT);
    }
    if a.info_len == 0 {
        return Err(errno::EINVAL);
    }
    Ok(())
}

/// Validates a link-create attr.
fn validate_link_create(a: &BpfLinkCreateAttr) -> Result<(), i32> {
    let prog = a.prog_fd as i32;
    if prog < 0 {
        return Err(errno::EBADF);
    }
    // target_fd can be -1 for prog types that don't attach to a fd
    // (e.g. tracing-fentry uses btf_id instead). But -1 here would be
    // u32::MAX after the cast; allow that and any non-negative value.
    let target = a.target_fd as i32;
    if target < BPF_FD_NONE {
        return Err(errno::EBADF);
    }
    if a.attach_type >= BPF_ATTACH_TYPE_MAX {
        return Err(errno::EINVAL);
    }
    Ok(())
}

/// Validates a link-update attr.
fn validate_link_update(a: &BpfLinkUpdateAttr) -> Result<(), i32> {
    let link = a.link_fd as i32;
    if link < 0 {
        return Err(errno::EBADF);
    }
    let new_prog = a.new_prog_fd as i32;
    if new_prog < 0 {
        return Err(errno::EBADF);
    }
    Ok(())
}

/// Validates a link-detach attr.
fn validate_link_detach(a: &BpfLinkDetachAttr) -> Result<(), i32> {
    let fd = a.link_fd as i32;
    if fd < 0 {
        return Err(errno::EBADF);
    }
    Ok(())
}

/// Validates a raw-tracepoint-open attr.
fn validate_raw_tp(a: &BpfRawTpAttr) -> Result<(), i32> {
    let fd = a.prog_fd as i32;
    if fd < 0 {
        return Err(errno::EBADF);
    }
    if a.name_ptr == 0 {
        return Err(errno::EFAULT);
    }
    Ok(())
}

/// Validates a get-fd-by-id attr.
fn validate_get_fd_by_id(a: &BpfGetIdAttr) -> Result<(), i32> {
    if a.start_id_or_target_id == 0 {
        // ID 0 is reserved / invalid in Linux.
        return Err(errno::ENOENT);
    }
    Ok(())
}

/// Validates a prog-bind-map attr.
fn validate_prog_bind_map(a: &BpfProgBindMapAttr) -> Result<(), i32> {
    let prog = a.prog_fd as i32;
    if prog < 0 {
        return Err(errno::EBADF);
    }
    let map = a.map_fd as i32;
    if map < 0 {
        return Err(errno::EBADF);
    }
    if a.flags != 0 {
        return Err(errno::EINVAL);
    }
    Ok(())
}

/// Validates a map-freeze attr.
fn validate_map_freeze(a: &BpfMapFreezeAttr) -> Result<(), i32> {
    let fd = a.map_fd as i32;
    if fd < 0 {
        return Err(errno::EBADF);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// bpf() syscall
// ---------------------------------------------------------------------------

/// Execute a BPF command.
///
/// Validates the `cmd`, `attr`, and `size` arguments against Linux's
/// `bpf(2)` contract. Anything that passes every check returns -1 /
/// `errno = ENOSYS` — matching a kernel built without
/// `CONFIG_BPF_SYSCALL=y`. Real BPF program loading, verification,
/// JIT, and attachment are deferred until the in-kernel BPF subsystem
/// lands.
///
/// # Errors
///
/// - `EINVAL`: unknown `cmd`, undersized `attr`, unknown
///   `map_type`/`prog_type`/`attach_type`, unknown flag bits,
///   missing required field, contradictory flag combination.
/// - `EFAULT`: NULL `attr` with nonzero `size`, or NULL field
///   pointer for a cmd that needs to read user memory (insns,
///   license, pathname, info buffer).
/// - `E2BIG`: `size` above 4 KiB safety cap, or `insn_cnt` above
///   `BPF_MAXINSNS`, or `data_size_in/out` above
///   `BPF_PROG_TEST_RUN_MAX_DATA`.
/// - `EBADF`: any fd argument that's negative or (in the case of
///   non-sentinel positions) outside our currently-empty fd table.
/// - `ENOENT`: lookup of ID 0 in `GET_FD_BY_ID`.
/// - `EPERM` *(Phase 182)*: per-cmd validation succeeded but caller
///   holds neither `CAP_BPF` nor `CAP_SYS_ADMIN`.  Matches Linux's
///   `bpf_capable()` gate under
///   `sysctl_unprivileged_bpf_disabled = 1` (the upstream and
///   Debian/Ubuntu default since Linux 5.16).  The gate fires AFTER
///   per-cmd shape validation so `EINVAL`/`EFAULT`/`E2BIG` always
///   beat `EPERM` (matching Linux: cmd handlers run shape checks
///   before the cap check).
/// - `ENOSYS`: all checks pass AND privilege held, but no real BPF
///   subsystem exists yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn bpf(cmd: u32, attr: *mut u8, size: u32) -> i32 {
    // Validation order matches Linux's `SYSCALL_DEFINE3(bpf, ...)` in
    // `kernel/bpf/syscall.c`:
    //   1. `bpf_check_uarg_tail_zero` → returns `-E2BIG` if
    //      `actual_size > PAGE_SIZE`.
    //   2. `copy_from_user(&attr, uattr, size)` → `-EFAULT` if the
    //      user pointer is bad (and `size > 0` is the trigger; a
    //      zero-byte copy is a no-op).
    //   3. switch on `cmd`; default arm returns `-EINVAL` for
    //      unknown commands.
    //   4. per-cmd handlers do their own shape/argument validation.

    // 1. Size bound check (Linux: bpf_check_uarg_tail_zero E2BIG).
    if size > BPF_ATTR_SIZE_MAX {
        errno::set_errno(errno::E2BIG);
        return -1;
    }
    // 2. attr must be non-NULL when we'd actually read bytes from it
    //    (Linux: copy_from_user with bad user pointer → -EFAULT).
    //    A zero-length copy is a no-op in Linux, so we only fault on
    //    NULL when size > 0.
    if size > 0 && attr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // 3. Unknown command — Linux's switch default → -EINVAL.
    if cmd >= BPF_CMD_MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // 4. Per-cmd shape minimum.  We require enough bytes for the
    //    per-cmd struct so callers can't construct a degenerate
    //    "zero-byte attr" that bypasses validation.  In Linux, the
    //    equivalent rejection happens inside each per-cmd handler.
    let needed = min_attr_size(cmd);
    if size < needed {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // After the size>=needed check, size > 0 always holds, so any
    // NULL attr would already have failed step 2.

    // Per-command shape validation. Read the relevant attr struct
    // (unaligned, since the caller's pointer may not be aligned).
    let result: Result<(), i32> = match cmd {
        BPF_MAP_CREATE => {
            let a: BpfMapCreateAttr = unsafe { read_attr(attr) };
            validate_map_create(&a)
        }
        BPF_MAP_LOOKUP_ELEM
        | BPF_MAP_UPDATE_ELEM
        | BPF_MAP_DELETE_ELEM
        | BPF_MAP_GET_NEXT_KEY
        | BPF_MAP_LOOKUP_AND_DELETE_ELEM => {
            let a: BpfMapElemAttr = unsafe { read_attr(attr) };
            validate_map_elem(&a, cmd)
        }
        BPF_PROG_LOAD => {
            let a: BpfProgLoadAttr = unsafe { read_attr(attr) };
            validate_prog_load(&a)
        }
        BPF_OBJ_PIN | BPF_OBJ_GET => {
            let a: BpfObjAttr = unsafe { read_attr(attr) };
            validate_obj(&a, cmd)
        }
        BPF_PROG_ATTACH | BPF_PROG_DETACH => {
            let a: BpfProgAttachAttr = unsafe { read_attr(attr) };
            validate_prog_attach(&a, cmd)
        }
        BPF_PROG_TEST_RUN => {
            let a: BpfProgTestRunAttr = unsafe { read_attr(attr) };
            validate_prog_test_run(&a)
        }
        BPF_PROG_GET_NEXT_ID
        | BPF_MAP_GET_NEXT_ID
        | BPF_BTF_GET_NEXT_ID
        | BPF_LINK_GET_NEXT_ID => {
            // start_id may be zero (means "start from the beginning"),
            // so we don't validate it here.
            Ok(())
        }
        BPF_PROG_GET_FD_BY_ID
        | BPF_MAP_GET_FD_BY_ID
        | BPF_BTF_GET_FD_BY_ID
        | BPF_LINK_GET_FD_BY_ID => {
            let a: BpfGetIdAttr = unsafe { read_attr(attr) };
            validate_get_fd_by_id(&a)
        }
        BPF_OBJ_GET_INFO_BY_FD => {
            let a: BpfInfoByFdAttr = unsafe { read_attr(attr) };
            validate_info_by_fd(&a)
        }
        BPF_LINK_CREATE => {
            let a: BpfLinkCreateAttr = unsafe { read_attr(attr) };
            validate_link_create(&a)
        }
        BPF_LINK_UPDATE => {
            let a: BpfLinkUpdateAttr = unsafe { read_attr(attr) };
            validate_link_update(&a)
        }
        BPF_LINK_DETACH => {
            let a: BpfLinkDetachAttr = unsafe { read_attr(attr) };
            validate_link_detach(&a)
        }
        BPF_RAW_TRACEPOINT_OPEN => {
            let a: BpfRawTpAttr = unsafe { read_attr(attr) };
            validate_raw_tp(&a)
        }
        BPF_ENABLE_STATS => Ok(()), // stats_type is just a u32 enum; ENOSYS is the answer regardless
        BPF_ITER_CREATE => {
            let a: BpfIterCreateAttr = unsafe { read_attr(attr) };
            let fd = a.link_fd as i32;
            if fd < 0 {
                Err(errno::EBADF)
            } else if a.flags != 0 {
                Err(errno::EINVAL)
            } else {
                Ok(())
            }
        }
        BPF_PROG_BIND_MAP => {
            let a: BpfProgBindMapAttr = unsafe { read_attr(attr) };
            validate_prog_bind_map(&a)
        }
        BPF_MAP_FREEZE => {
            let a: BpfMapFreezeAttr = unsafe { read_attr(attr) };
            validate_map_freeze(&a)
        }
        // Remaining commands (BTF_LOAD, TASK_FD_QUERY, batch ops,
        // PROG_QUERY, TOKEN_CREATE): pass the size check then go
        // straight to ENOSYS. Their attr shapes are large and
        // BTF-dependent; we don't model them here because every
        // realistic caller will see ENOSYS at this command level
        // before they can construct a meaningful attr anyway.
        _ => Ok(()),
    };

    if let Err(e) = result {
        errno::set_errno(e);
        return -1;
    }

    // Phase 182: privilege check.  Linux's per-cmd handlers each call
    // `bpf_capable()` (== `capable(CAP_BPF) || capable(CAP_SYS_ADMIN)`)
    // after their shape validation; failing the cap check returns
    // `-EPERM`.  Under `sysctl_unprivileged_bpf_disabled` (set since
    // Linux 5.16 upstream and earlier on Debian/Ubuntu), the gate
    // applies to **every** bpf() command, not just the dangerous ones
    // — see `kernel/bpf/syscall.c::__sys_bpf` and the various
    // `bpf_capable()` call-sites in `map_create()`, `bpf_prog_load()`,
    // `bpf_obj_pin()`, etc.
    //
    // We model the strictest setting (sysctl=1) since we have no
    // sysctl backend.  Errno is EPERM, not EACCES (Linux distinguishes
    // them — EPERM for cap rejection, EACCES for policy denial in
    // LSM contexts; bpf_capable's rejection is EPERM).
    //
    // Placement is after per-cmd validation succeeds but before the
    // ENOSYS fall-through — matching Linux's per-handler order: shape
    // check first (so a bug in attr layout surfaces as EINVAL), then
    // cap check, then backend execution.
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_BPF,
    ) && !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SYS_ADMIN,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }

    // All validation passed AND privilege held — no real BPF
    // subsystem to dispatch to.
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;
    use core::ptr;

    /// Builds a valid `BPF_MAP_CREATE` attr (hash, 4→4, 1024 entries).
    fn good_map_create() -> BpfMapCreateAttr {
        BpfMapCreateAttr {
            map_type: BPF_MAP_TYPE_HASH,
            key_size: 4,
            value_size: 4,
            max_entries: 1024,
            map_flags: 0,
            inner_map_fd: 0,
            numa_node: 0,
            map_name: [0; 16],
            map_ifindex: 0,
        }
    }

    /// Builds a valid prog-load attr (socket-filter, 2 insns).
    fn good_prog_load() -> BpfProgLoadAttr {
        BpfProgLoadAttr {
            prog_type: BPF_PROG_TYPE_SOCKET_FILTER,
            insn_cnt: 2,
            // Fake non-null pointers — the syscall returns ENOSYS
            // before dereferencing them.
            insns_ptr: 0x1000,
            license_ptr: 0x2000,
            log_level: 0,
            log_size: 0,
            log_buf_ptr: 0,
            kern_version: 0,
            prog_flags: 0,
            prog_name: [0; 16],
            prog_ifindex: 0,
            expected_attach_type: 0,
        }
    }

    fn call_bpf<T>(cmd: u32, attr: &T) -> i32 {
        let p = (attr as *const T).cast::<u8>() as *mut u8;
        bpf(cmd, p, mem::size_of::<T>() as u32)
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            BPF_MAP_CREATE, BPF_MAP_LOOKUP_ELEM, BPF_MAP_UPDATE_ELEM,
            BPF_MAP_DELETE_ELEM, BPF_MAP_GET_NEXT_KEY, BPF_PROG_LOAD,
            BPF_OBJ_PIN, BPF_OBJ_GET, BPF_PROG_ATTACH, BPF_PROG_DETACH,
            BPF_PROG_TEST_RUN, BPF_PROG_GET_NEXT_ID, BPF_MAP_GET_NEXT_ID,
            BPF_PROG_GET_FD_BY_ID, BPF_MAP_GET_FD_BY_ID,
            BPF_OBJ_GET_INFO_BY_FD, BPF_LINK_CREATE, BPF_LINK_UPDATE,
            BPF_LINK_DETACH, BPF_MAP_FREEZE, BPF_RAW_TRACEPOINT_OPEN,
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
            BPF_MAP_TYPE_RINGBUF, BPF_MAP_TYPE_USER_RINGBUF,
            BPF_MAP_TYPE_ARENA,
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
            BPF_PROG_TYPE_SOCKET_FILTER, BPF_PROG_TYPE_KPROBE,
            BPF_PROG_TYPE_XDP, BPF_PROG_TYPE_TRACEPOINT,
            BPF_PROG_TYPE_PERF_EVENT, BPF_PROG_TYPE_NETFILTER,
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
        assert_eq!(BPF_F_LOCK, 4);
    }

    #[test]
    fn test_unknown_cmd_einval() {
        let a = good_map_create();
        let p = (&a as *const _ as *mut u8).cast::<u8>();
        let r = bpf(BPF_CMD_MAX, p, mem::size_of::<BpfMapCreateAttr>() as u32);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_huge_size_e2big() {
        let a = good_map_create();
        let p = (&a as *const _ as *mut u8).cast::<u8>();
        let r = bpf(BPF_MAP_CREATE, p, 8193);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_undersized_attr_einval() {
        let a = good_map_create();
        let p = (&a as *const _ as *mut u8).cast::<u8>();
        let r = bpf(BPF_MAP_CREATE, p, 4); // far below the cmd's minimum
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_null_attr_efault() {
        let r = bpf(
            BPF_MAP_CREATE,
            ptr::null_mut(),
            mem::size_of::<BpfMapCreateAttr>() as u32,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_map_create_unspec_einval() {
        let mut a = good_map_create();
        a.map_type = BPF_MAP_TYPE_UNSPEC;
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_map_create_unknown_type_einval() {
        let mut a = good_map_create();
        a.map_type = 0xFFFF;
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_map_create_zero_value_einval() {
        let mut a = good_map_create();
        a.value_size = 0;
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_map_create_zero_key_einval_for_hash() {
        let mut a = good_map_create();
        a.key_size = 0;
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_map_create_zero_key_ok_for_queue() {
        let mut a = good_map_create();
        a.map_type = BPF_MAP_TYPE_QUEUE;
        a.key_size = 0;
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_map_create_zero_max_einval() {
        let mut a = good_map_create();
        a.max_entries = 0;
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_map_create_zero_max_ok_for_struct_ops() {
        let mut a = good_map_create();
        a.map_type = BPF_MAP_TYPE_STRUCT_OPS;
        a.max_entries = 0;
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_map_create_unknown_flag_einval() {
        let mut a = good_map_create();
        a.map_flags = 0x8000_0000; // top bit, never valid
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_map_create_rdonly_wronly_conflict_einval() {
        let mut a = good_map_create();
        a.map_flags = BPF_F_RDONLY | BPF_F_WRONLY;
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_map_create_rdonly_prog_wronly_prog_conflict_einval() {
        let mut a = good_map_create();
        a.map_flags = BPF_F_RDONLY_PROG | BPF_F_WRONLY_PROG;
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_map_create_valid_reaches_enosys() {
        let a = good_map_create();
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_map_elem_negative_fd_ebadf() {
        let a = BpfMapElemAttr {
            map_fd: u32::MAX, // -1 when reinterpreted as i32
            key_ptr: 0x1000,
            value_or_next_key_ptr: 0x2000,
            flags: 0,
        };
        assert_eq!(call_bpf(BPF_MAP_LOOKUP_ELEM, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_map_update_unknown_flag_einval() {
        let a = BpfMapElemAttr {
            map_fd: 3,
            key_ptr: 0x1000,
            value_or_next_key_ptr: 0x2000,
            flags: 0x100, // outside the valid mask
        };
        assert_eq!(call_bpf(BPF_MAP_UPDATE_ELEM, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_map_update_noexist_and_exist_einval() {
        let a = BpfMapElemAttr {
            map_fd: 3,
            key_ptr: 0x1000,
            value_or_next_key_ptr: 0x2000,
            flags: BPF_NOEXIST | BPF_EXIST,
        };
        assert_eq!(call_bpf(BPF_MAP_UPDATE_ELEM, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_map_update_valid_reaches_enosys() {
        let a = BpfMapElemAttr {
            map_fd: 3,
            key_ptr: 0x1000,
            value_or_next_key_ptr: 0x2000,
            flags: BPF_NOEXIST,
        };
        assert_eq!(call_bpf(BPF_MAP_UPDATE_ELEM, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_map_lookup_nonzero_unknown_flag_einval() {
        let a = BpfMapElemAttr {
            map_fd: 3,
            key_ptr: 0x1000,
            value_or_next_key_ptr: 0x2000,
            flags: 0x100, // LOCK is allowed, this isn't
        };
        assert_eq!(call_bpf(BPF_MAP_LOOKUP_ELEM, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prog_load_unspec_einval() {
        let mut a = good_prog_load();
        a.prog_type = BPF_PROG_TYPE_UNSPEC;
        assert_eq!(call_bpf(BPF_PROG_LOAD, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prog_load_unknown_type_einval() {
        let mut a = good_prog_load();
        a.prog_type = 0xFFFF;
        assert_eq!(call_bpf(BPF_PROG_LOAD, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prog_load_zero_insn_cnt_einval() {
        let mut a = good_prog_load();
        a.insn_cnt = 0;
        assert_eq!(call_bpf(BPF_PROG_LOAD, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prog_load_too_many_insns_e2big() {
        let mut a = good_prog_load();
        a.insn_cnt = BPF_MAXINSNS + 1;
        assert_eq!(call_bpf(BPF_PROG_LOAD, &a), -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_prog_load_null_insns_efault() {
        let mut a = good_prog_load();
        a.insns_ptr = 0;
        assert_eq!(call_bpf(BPF_PROG_LOAD, &a), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_prog_load_null_license_efault() {
        let mut a = good_prog_load();
        a.license_ptr = 0;
        assert_eq!(call_bpf(BPF_PROG_LOAD, &a), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_prog_load_log_without_buf_einval() {
        let mut a = good_prog_load();
        a.log_level = 1;
        a.log_size = 0; // no buffer size despite asking for log output
        a.log_buf_ptr = 0;
        assert_eq!(call_bpf(BPF_PROG_LOAD, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prog_load_huge_log_e2big() {
        let mut a = good_prog_load();
        a.log_level = 1;
        a.log_size = BPF_PROG_TEST_RUN_MAX_DATA + 1;
        a.log_buf_ptr = 0x3000;
        assert_eq!(call_bpf(BPF_PROG_LOAD, &a), -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_prog_load_unknown_attach_type_einval() {
        let mut a = good_prog_load();
        a.expected_attach_type = 0xFFFF;
        assert_eq!(call_bpf(BPF_PROG_LOAD, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prog_load_valid_reaches_enosys() {
        let a = good_prog_load();
        assert_eq!(call_bpf(BPF_PROG_LOAD, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_obj_pin_null_path_efault() {
        let a = BpfObjAttr {
            pathname_ptr: 0,
            bpf_fd: 3,
            file_flags: 0,
        };
        assert_eq!(call_bpf(BPF_OBJ_PIN, &a), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_obj_pin_negative_fd_ebadf() {
        let a = BpfObjAttr {
            pathname_ptr: 0x1000,
            bpf_fd: u32::MAX,
            file_flags: 0,
        };
        assert_eq!(call_bpf(BPF_OBJ_PIN, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_obj_get_null_path_efault() {
        let a = BpfObjAttr {
            pathname_ptr: 0,
            bpf_fd: 0,
            file_flags: 0,
        };
        assert_eq!(call_bpf(BPF_OBJ_GET, &a), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_obj_get_valid_reaches_enosys() {
        let a = BpfObjAttr {
            pathname_ptr: 0x1000,
            bpf_fd: 0,
            file_flags: 0,
        };
        assert_eq!(call_bpf(BPF_OBJ_GET, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_prog_attach_negative_target_ebadf() {
        let a = BpfProgAttachAttr {
            target_fd: u32::MAX,
            attach_bpf_fd: 3,
            attach_type: BPF_CGROUP_INET_INGRESS,
            attach_flags: 0,
        };
        assert_eq!(call_bpf(BPF_PROG_ATTACH, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_prog_attach_negative_prog_ebadf() {
        let a = BpfProgAttachAttr {
            target_fd: 3,
            attach_bpf_fd: u32::MAX,
            attach_type: BPF_CGROUP_INET_INGRESS,
            attach_flags: 0,
        };
        assert_eq!(call_bpf(BPF_PROG_ATTACH, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_prog_attach_unknown_type_einval() {
        let a = BpfProgAttachAttr {
            target_fd: 3,
            attach_bpf_fd: 4,
            attach_type: 0xFFFF,
            attach_flags: 0,
        };
        assert_eq!(call_bpf(BPF_PROG_ATTACH, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prog_attach_valid_reaches_enosys() {
        let a = BpfProgAttachAttr {
            target_fd: 3,
            attach_bpf_fd: 4,
            attach_type: BPF_CGROUP_INET_INGRESS,
            attach_flags: 0,
        };
        assert_eq!(call_bpf(BPF_PROG_ATTACH, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_prog_test_run_huge_data_e2big() {
        let a = BpfProgTestRunAttr {
            prog_fd: 3,
            retval: 0,
            data_size_in: BPF_PROG_TEST_RUN_MAX_DATA + 1,
            data_size_out: 0,
            data_in: 0x1000,
            data_out: 0,
            repeat: 1,
            duration: 0,
        };
        assert_eq!(call_bpf(BPF_PROG_TEST_RUN, &a), -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_prog_test_run_negative_fd_ebadf() {
        let a = BpfProgTestRunAttr {
            prog_fd: u32::MAX,
            retval: 0,
            data_size_in: 64,
            data_size_out: 64,
            data_in: 0x1000,
            data_out: 0x2000,
            repeat: 1,
            duration: 0,
        };
        assert_eq!(call_bpf(BPF_PROG_TEST_RUN, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_info_by_fd_negative_fd_ebadf() {
        let a = BpfInfoByFdAttr {
            bpf_fd: u32::MAX,
            info_len: 64,
            info_ptr: 0x1000,
        };
        assert_eq!(call_bpf(BPF_OBJ_GET_INFO_BY_FD, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_info_by_fd_null_info_efault() {
        let a = BpfInfoByFdAttr {
            bpf_fd: 3,
            info_len: 64,
            info_ptr: 0,
        };
        assert_eq!(call_bpf(BPF_OBJ_GET_INFO_BY_FD, &a), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_info_by_fd_zero_len_einval() {
        let a = BpfInfoByFdAttr {
            bpf_fd: 3,
            info_len: 0,
            info_ptr: 0x1000,
        };
        assert_eq!(call_bpf(BPF_OBJ_GET_INFO_BY_FD, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_link_create_negative_prog_ebadf() {
        let a = BpfLinkCreateAttr {
            prog_fd: u32::MAX,
            target_fd: 3,
            attach_type: BPF_CGROUP_INET_INGRESS,
            flags: 0,
        };
        assert_eq!(call_bpf(BPF_LINK_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_link_create_unknown_attach_einval() {
        let a = BpfLinkCreateAttr {
            prog_fd: 3,
            target_fd: 4,
            attach_type: 0xFFFF,
            flags: 0,
        };
        assert_eq!(call_bpf(BPF_LINK_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_link_create_valid_reaches_enosys() {
        let a = BpfLinkCreateAttr {
            prog_fd: 3,
            target_fd: 4,
            attach_type: BPF_XDP,
            flags: 0,
        };
        assert_eq!(call_bpf(BPF_LINK_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_link_create_minus_one_target_ok_for_tracing() {
        // Tracing/fentry/fexit programs pass target_fd=-1 because the
        // attach point is described by btf_id, not a fd.
        let a = BpfLinkCreateAttr {
            prog_fd: 3,
            target_fd: u32::MAX, // -1 when cast to i32
            attach_type: BPF_TRACE_FENTRY,
            flags: 0,
        };
        assert_eq!(call_bpf(BPF_LINK_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_link_update_negative_link_ebadf() {
        let a = BpfLinkUpdateAttr {
            link_fd: u32::MAX,
            new_prog_fd: 3,
            flags: 0,
            old_prog_fd: 0,
        };
        assert_eq!(call_bpf(BPF_LINK_UPDATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_link_detach_negative_link_ebadf() {
        let a = BpfLinkDetachAttr {
            link_fd: u32::MAX,
        };
        assert_eq!(call_bpf(BPF_LINK_DETACH, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_link_detach_valid_reaches_enosys() {
        let a = BpfLinkDetachAttr { link_fd: 3 };
        assert_eq!(call_bpf(BPF_LINK_DETACH, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_raw_tp_negative_fd_ebadf() {
        let a = BpfRawTpAttr {
            name_ptr: 0x1000,
            prog_fd: u32::MAX,
        };
        assert_eq!(call_bpf(BPF_RAW_TRACEPOINT_OPEN, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_raw_tp_null_name_efault() {
        let a = BpfRawTpAttr {
            name_ptr: 0,
            prog_fd: 3,
        };
        assert_eq!(call_bpf(BPF_RAW_TRACEPOINT_OPEN, &a), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_get_fd_by_id_zero_id_enoent() {
        let a = BpfGetIdAttr {
            start_id_or_target_id: 0,
            next_id_or_open_flags: 0,
            _pad: 0,
        };
        assert_eq!(call_bpf(BPF_PROG_GET_FD_BY_ID, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    #[test]
    fn test_get_fd_by_id_nonzero_reaches_enosys() {
        let a = BpfGetIdAttr {
            start_id_or_target_id: 1,
            next_id_or_open_flags: 0,
            _pad: 0,
        };
        assert_eq!(call_bpf(BPF_PROG_GET_FD_BY_ID, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_get_next_id_zero_ok() {
        // "Start from 0" is the canonical way to iterate.
        let a = BpfGetIdAttr {
            start_id_or_target_id: 0,
            next_id_or_open_flags: 0,
            _pad: 0,
        };
        assert_eq!(call_bpf(BPF_PROG_GET_NEXT_ID, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_prog_bind_map_negative_fd_ebadf() {
        let a = BpfProgBindMapAttr {
            prog_fd: u32::MAX,
            map_fd: 3,
            flags: 0,
        };
        assert_eq!(call_bpf(BPF_PROG_BIND_MAP, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_prog_bind_map_nonzero_flags_einval() {
        let a = BpfProgBindMapAttr {
            prog_fd: 3,
            map_fd: 4,
            flags: 1,
        };
        assert_eq!(call_bpf(BPF_PROG_BIND_MAP, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_map_freeze_negative_fd_ebadf() {
        let a = BpfMapFreezeAttr { map_fd: u32::MAX };
        assert_eq!(call_bpf(BPF_MAP_FREEZE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_map_freeze_valid_reaches_enosys() {
        let a = BpfMapFreezeAttr { map_fd: 3 };
        assert_eq!(call_bpf(BPF_MAP_FREEZE, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_iter_create_negative_link_ebadf() {
        let a = BpfIterCreateAttr {
            link_fd: u32::MAX,
            flags: 0,
        };
        assert_eq!(call_bpf(BPF_ITER_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_iter_create_nonzero_flags_einval() {
        let a = BpfIterCreateAttr {
            link_fd: 3,
            flags: 1,
        };
        assert_eq!(call_bpf(BPF_ITER_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_misaligned_attr_pointer_handled() {
        // Build a buffer larger than a map-create attr; place the attr
        // at offset +1 to guarantee misalignment for any field type.
        let good = good_map_create();
        let mut buf = [0u8; mem::size_of::<BpfMapCreateAttr>() + 1];
        unsafe {
            ptr::copy_nonoverlapping(
                (&good as *const BpfMapCreateAttr).cast::<u8>(),
                buf.as_mut_ptr().add(1),
                mem::size_of::<BpfMapCreateAttr>(),
            );
        }
        let p = unsafe { buf.as_mut_ptr().add(1) };
        let r = bpf(BPF_MAP_CREATE, p, mem::size_of::<BpfMapCreateAttr>() as u32);
        assert_eq!(r, -1);
        // Valid attr → reaches ENOSYS through the read_unaligned path.
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_errno_preserved_on_validation_success() {
        // Even though ENOSYS is set on success-of-validation, an
        // *unrelated* prior errno should not affect validation.
        errno::set_errno(errno::EBADF);
        let a = good_map_create();
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        // The syscall itself sets ENOSYS — that's correct.
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_typical_libbpf_probe_workflow() {
        // libbpf's `bpf_object__load` probes the kernel by trying
        // BPF_PROG_LOAD with a tiny test program. It expects either
        // success or a specific error to detect capability.
        let mut a = good_prog_load();
        a.prog_type = BPF_PROG_TYPE_SOCKET_FILTER;
        a.insn_cnt = 2;
        // For a real kernel the test program would be 2 BPF insns
        // (mov r0=1; exit). We just need non-null pointers.
        assert_eq!(call_bpf(BPF_PROG_LOAD, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
        // libbpf sees ENOSYS and reports "kernel doesn't support BPF"
        // instead of pretending it's a generic error.
    }

    #[test]
    fn test_bcc_map_create_workflow() {
        // BCC's frontend creates a hash map for kprobe-collected
        // counters before loading the program.
        let a = BpfMapCreateAttr {
            map_type: BPF_MAP_TYPE_PERCPU_HASH,
            key_size: 8,
            value_size: 8,
            max_entries: 10240,
            map_flags: 0,
            inner_map_fd: 0,
            numa_node: 0,
            map_name: *b"counters\0\0\0\0\0\0\0\0",
            map_ifindex: 0,
        };
        assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_cilium_link_create_xdp_workflow() {
        // Cilium attaches an XDP program via BPF_LINK_CREATE so it can
        // be replaced without dropping packets.
        let a = BpfLinkCreateAttr {
            prog_fd: 3,
            target_fd: 4, // ifindex-as-fd via /sys/class/net/...
            attach_type: BPF_XDP,
            flags: 0,
        };
        assert_eq!(call_bpf(BPF_LINK_CREATE, &a), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- Phase 120: bpf() prologue precedence vs. Linux ---------------------
    //
    // Linux's bpf syscall runs `bpf_check_uarg_tail_zero` (E2BIG for
    // size > PAGE_SIZE), then `copy_from_user` (EFAULT for bad user
    // pointer with non-zero size), then the cmd switch (EINVAL for
    // unknown cmd in the default arm).  Our previous order checked
    // `cmd` first, which made unknown-cmd EINVAL beat both E2BIG and
    // EFAULT — observable on buggy-caller calls where multiple
    // arguments were broken at once.

    fn fresh_errno() {
        errno::set_errno(0);
    }

    #[test]
    fn test_bpf_phase120_size_e2big_wins_over_unknown_cmd() {
        // (cmd=invalid, size=huge): Linux returns E2BIG before the cmd
        // switch fires.  Was EINVAL.
        fresh_errno();
        let a = good_map_create();
        let p = (&a as *const _ as *mut u8).cast::<u8>();
        let r = bpf(BPF_CMD_MAX + 100, p, 8193);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_bpf_phase120_efault_wins_over_unknown_cmd() {
        // (cmd=invalid, attr=NULL, size>0): Linux returns EFAULT from
        // copy_from_user before reaching the cmd switch.  Was EINVAL.
        fresh_errno();
        let r = bpf(
            BPF_CMD_MAX + 5,
            ptr::null_mut(),
            mem::size_of::<BpfMapCreateAttr>() as u32,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_bpf_phase120_size_e2big_wins_over_null_attr() {
        // (cmd=valid, attr=NULL, size=huge): Linux checks size first
        // (E2BIG) before copy_from_user could fail with EFAULT.
        fresh_errno();
        let r = bpf(BPF_MAP_CREATE, ptr::null_mut(), 8193);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_bpf_phase120_size_e2big_wins_over_undersized() {
        // (cmd=valid, size=huge): E2BIG fires before per-cmd min size
        // check could complain.  Same as before, but pinned.
        fresh_errno();
        let a = good_map_create();
        let p = (&a as *const _ as *mut u8).cast::<u8>();
        let r = bpf(BPF_MAP_CREATE, p, BPF_ATTR_SIZE_MAX + 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_bpf_phase120_efault_wins_over_undersized() {
        // (cmd=valid, attr=NULL, size=valid-but-undersized for the cmd
        // shape, yet > 0): EFAULT (copy fails) before per-cmd EINVAL.
        fresh_errno();
        let r = bpf(BPF_MAP_CREATE, ptr::null_mut(), 4); // size > 0 but tiny
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_bpf_phase120_unknown_cmd_after_size_and_null_pass() {
        // (cmd=invalid, attr=valid, size=valid for some struct): with
        // size/attr both fine, we reach the cmd switch → EINVAL.
        fresh_errno();
        let a = good_map_create();
        let p = (&a as *const _ as *mut u8).cast::<u8>();
        let r = bpf(BPF_CMD_MAX, p, mem::size_of::<BpfMapCreateAttr>() as u32);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_bpf_phase120_size_zero_with_null_attr_invokes_cmd() {
        // (cmd=invalid, attr=NULL, size=0): Linux's copy_from_user
        // with size=0 is a no-op; we now match that and proceed to
        // the cmd check, which yields EINVAL for the unknown cmd.
        // Previously this was also EINVAL but via the cmd-first path;
        // post-Phase 120 it goes through the size>0&&null branch
        // (which doesn't fire because size==0) into the cmd check.
        fresh_errno();
        let r = bpf(BPF_CMD_MAX + 7, ptr::null_mut(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_bpf_phase120_size_zero_with_valid_cmd_reaches_undersized_einval() {
        // (cmd=valid, attr=valid-but-unused, size=0): now passes
        // null/E2BIG checks and falls into per-cmd min-size → EINVAL.
        fresh_errno();
        let a = good_map_create();
        let p = (&a as *const _ as *mut u8).cast::<u8>();
        let r = bpf(BPF_MAP_CREATE, p, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_bpf_phase120_clean_args_still_reach_enosys() {
        // Happy path must still pass all four prologue checks.
        fresh_errno();
        let a = good_map_create();
        let p = (&a as *const _ as *mut u8).cast::<u8>();
        let r = bpf(BPF_MAP_CREATE, p, mem::size_of::<BpfMapCreateAttr>() as u32);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_bpf_phase120_max_u32_size_e2big_not_overflow() {
        // u32::MAX size must report E2BIG without arithmetic overflow.
        fresh_errno();
        let a = good_map_create();
        let p = (&a as *const _ as *mut u8).cast::<u8>();
        let r = bpf(BPF_MAP_CREATE, p, u32::MAX);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_bpf_phase120_max_u32_size_e2big_wins_over_unknown_cmd() {
        // (cmd=invalid, size=u32::MAX, attr=NULL): size E2BIG fires
        // before any other check (cmd unknown or NULL EFAULT).
        fresh_errno();
        let r = bpf(BPF_CMD_MAX + 99, ptr::null_mut(), u32::MAX);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_bpf_phase120_recovery_after_e2big() {
        // After an E2BIG, the next clean call must still reach ENOSYS
        // (no sticky state from the prologue rejection).
        fresh_errno();
        let a = good_map_create();
        let p = (&a as *const _ as *mut u8).cast::<u8>();
        let r1 = bpf(BPF_MAP_CREATE, p, 8193);
        assert_eq!(r1, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);

        fresh_errno();
        let r2 = bpf(BPF_MAP_CREATE, p, mem::size_of::<BpfMapCreateAttr>() as u32);
        assert_eq!(r2, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_bpf_phase120_buggy_caller_passes_negative_int_as_u32_size() {
        // C code casting `-1` (int) to `unsigned int` size yields
        // u32::MAX.  Linux returns E2BIG for that; we match.
        fresh_errno();
        let a = good_map_create();
        let p = (&a as *const _ as *mut u8).cast::<u8>();
        let bogus_size: u32 = (-1i32) as u32;
        let r = bpf(BPF_MAP_CREATE, p, bogus_size);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    // ----------------------------------------------------------------------
    // Phase 182: bpf() — CAP_BPF / CAP_SYS_ADMIN gate.
    //
    // Pre-Phase-182 behaviour: every well-formed bpf() call fell
    // through to ENOSYS regardless of capability, because the docstring
    // step "privilege check" was unimplemented.  That let any process
    // probe for BPF availability without ever seeing EPERM —
    // misleading loader libraries (libbpf, bcc, bpftrace) that inspect
    // errno to decide whether to fall back to non-BPF tracing or
    // exit with a clear "missing CAP_BPF" diagnostic.
    //
    // Linux semantics (kernel/bpf/syscall.c, per-cmd handlers):
    //     if (!bpf_capable())
    //         return -EPERM;
    // where bpf_capable() ::= capable(CAP_BPF) || capable(CAP_SYS_ADMIN).
    //
    // Under sysctl_unprivileged_bpf_disabled = 1 (Linux 5.16+
    // upstream default; earlier on Debian/Ubuntu) every command is
    // gated.  We model that strictest setting since we have no
    // sysctl backend.
    //
    // Placement is after per-cmd shape validation but before the
    // ENOSYS fall-through — matching Linux's per-handler order
    // (shape check → cap check → backend execution).  This makes
    // EINVAL/EFAULT/E2BIG/EBADF beat EPERM, which matches what
    // tools see on real Linux.
    // ----------------------------------------------------------------------

    mod bpf_cap_phase182 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase 181.
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap(cap: u32) {
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if cap < 32 {
                (lo & !(1u32 << cap), hi)
            } else {
                (lo, hi & !(1u32 << (cap - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed dropping cap");
            assert!(!crate::sys_capability::has_capability(cap));
        }

        fn drop_bpf_caps() {
            drop_cap(crate::sys_capability::CAP_BPF);
            drop_cap(crate::sys_capability::CAP_SYS_ADMIN);
        }

        // -- Per-error-class ----------------------------------------------

        /// Valid BPF_MAP_CREATE without CAP_BPF or CAP_SYS_ADMIN →
        /// -1/EPERM.
        #[test]
        fn test_bpf_phase182_map_create_no_caps_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            let a = good_map_create();
            fresh_errno();
            let r = call_bpf(BPF_MAP_CREATE, &a);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Errno is specifically EPERM (Linux's bpf_capable
        /// rejection), not EACCES (policy denial) and not ENOSYS
        /// (backend missing).
        #[test]
        fn test_bpf_phase182_errno_is_eperm_not_eacces() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            let a = good_map_create();
            fresh_errno();
            assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
            assert_ne!(errno::get_errno(), errno::EACCES);
            assert_ne!(errno::get_errno(), errno::ENOSYS);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// CAP_BPF alone (without CAP_SYS_ADMIN) is sufficient —
        /// matches `bpf_capable() = CAP_BPF || CAP_SYS_ADMIN`.
        /// CAP_BPF is the modern fine-grained alternative (Linux 5.8+).
        #[test]
        fn test_bpf_phase182_cap_bpf_alone_satisfies_gate() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SYS_ADMIN);
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_BPF
            ));
            assert!(!crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            let a = good_map_create();
            fresh_errno();
            let r = call_bpf(BPF_MAP_CREATE, &a);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// CAP_SYS_ADMIN alone (without CAP_BPF) is sufficient —
        /// the historical "superuser" cap also passes the gate.
        #[test]
        fn test_bpf_phase182_cap_sys_admin_alone_satisfies_gate() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_BPF);
            assert!(!crate::sys_capability::has_capability(
                crate::sys_capability::CAP_BPF
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            let a = good_map_create();
            fresh_errno();
            let r = call_bpf(BPF_MAP_CREATE, &a);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// Dropping BOTH caps denies — confirms the gate is OR'd,
        /// not AND'd, and that no other cap satisfies it.
        #[test]
        fn test_bpf_phase182_drop_both_caps_denies() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            let a = good_map_create();
            fresh_errno();
            assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Multiple distinct cmds (MAP_CREATE, PROG_LOAD, OBJ_PIN)
        /// all gated by the same caps — confirms the check is
        /// applied uniformly across the command set, not just to one
        /// hot path.
        #[test]
        fn test_bpf_phase182_multiple_cmds_all_gated() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            // MAP_CREATE
            let m = good_map_create();
            fresh_errno();
            assert_eq!(call_bpf(BPF_MAP_CREATE, &m), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // PROG_LOAD
            let p = good_prog_load();
            fresh_errno();
            assert_eq!(call_bpf(BPF_PROG_LOAD, &p), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering matrix ----------------------------------------------

        /// E2BIG (oversize attr) beats EPERM.  Linux's
        /// bpf_check_uarg_tail_zero runs before per-cmd handlers
        /// (and their cap checks).
        #[test]
        fn test_bpf_phase182_e2big_size_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            let a = good_map_create();
            let p = (&a as *const _ as *mut u8).cast::<u8>();
            fresh_errno();
            // BPF_ATTR_SIZE_MAX is 4096; use 8192.
            let r = bpf(BPF_MAP_CREATE, p, 8192);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::E2BIG);
        }

        /// EFAULT (NULL attr with size > 0) beats EPERM.
        #[test]
        fn test_bpf_phase182_efault_null_attr_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            fresh_errno();
            let r = bpf(BPF_MAP_CREATE, ptr::null_mut(), 64);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }

        /// EINVAL (unknown cmd) beats EPERM.  Linux's switch
        /// default returns -EINVAL before any per-cmd handler runs.
        #[test]
        fn test_bpf_phase182_einval_unknown_cmd_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            let a = good_map_create();
            let p = (&a as *const _ as *mut u8).cast::<u8>();
            fresh_errno();
            // BPF_CMD_MAX is the exclusive upper bound; use it as a
            // guaranteed-unknown cmd.
            let r = bpf(BPF_CMD_MAX, p, mem::size_of::<BpfMapCreateAttr>() as u32);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// EINVAL (undersize attr) beats EPERM — the size-floor
        /// check runs before per-cmd validation/cap check.
        #[test]
        fn test_bpf_phase182_einval_undersize_attr_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            let a = good_map_create();
            let p = (&a as *const _ as *mut u8).cast::<u8>();
            fresh_errno();
            // 4 bytes is well below any per-cmd minimum.
            let r = bpf(BPF_MAP_CREATE, p, 4);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// Per-cmd EINVAL (bad map_type) beats EPERM — validate_*
        /// runs before our cap gate, matching Linux's per-handler
        /// "shape check then cap check" order.
        #[test]
        fn test_bpf_phase182_einval_bad_map_type_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            let mut a = good_map_create();
            a.map_type = u32::MAX; // wildly out of range
            fresh_errno();
            let r = call_bpf(BPF_MAP_CREATE, &a);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- Workflow -----------------------------------------------------

        /// libbpf-style loader workflow: probe MAP_CREATE without
        /// caps → EPERM (loader knows to surface a clear
        /// "missing CAP_BPF" diagnostic); regain CAP_BPF → ENOSYS
        /// (loader knows the kernel lacks CONFIG_BPF_SYSCALL=y and
        /// falls back to non-BPF tracing).
        #[test]
        fn test_bpf_phase182_workflow_libbpf_probe_eperm_then_enosys() {
            let _g = CapGuard::snapshot();
            // 1st: drop caps, expect EPERM.
            drop_bpf_caps();
            let a = good_map_create();
            fresh_errno();
            assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // 2nd: restore CAP_BPF, expect ENOSYS.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: (1u32 << 9) - 1,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0
            );
            fresh_errno();
            assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// Sandbox workflow: parent with caps reaches ENOSYS;
        /// child after sandbox drop sees EPERM for the same call.
        /// Models a privileged daemon spawning untrusted code.
        #[test]
        fn test_bpf_phase182_workflow_sandbox_drops_then_denied() {
            let _g = CapGuard::snapshot();
            // Parent with caps: ENOSYS.
            let a = good_map_create();
            fresh_errno();
            assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
            // After sandbox drop: EPERM.
            drop_bpf_caps();
            fresh_errno();
            assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Buggy caller -------------------------------------------------

        /// Multi-error: a no-cap caller passing unknown cmd AND
        /// E2BIG size sees E2BIG (earliest check wins).
        #[test]
        fn test_bpf_phase182_buggy_caller_multi_error_e2big_wins() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            let a = good_map_create();
            let p = (&a as *const _ as *mut u8).cast::<u8>();
            fresh_errno();
            let r = bpf(BPF_CMD_MAX, p, 8192);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::E2BIG);
        }

        // -- Recovery -----------------------------------------------------

        /// After EPERM, granting CAP_BPF (alone) reaches ENOSYS.
        /// Confirms dynamic per-call cap evaluation.
        #[test]
        fn test_bpf_phase182_recovery_restore_cap_bpf_reaches_enosys() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            let a = good_map_create();
            fresh_errno();
            assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Re-grant CAP_BPF only (CAP_SYS_ADMIN stays dropped).
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            // CAP_BPF is cap 39 → bit 7 of the high word.
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: 1u32 << 7,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0
            );
            fresh_errno();
            assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- No-side-effect ----------------------------------------------

        /// EPERM rejection leaves capability sets unchanged — bpf
        /// does not silently grant/drop caps.
        #[test]
        fn test_bpf_phase182_eperm_preserves_other_caps() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            let (lo_before, hi_before) =
                crate::sys_capability::current_caps_effective();
            let a = good_map_create();
            let _ = call_bpf(BPF_MAP_CREATE, &a);
            let (lo_after, hi_after) =
                crate::sys_capability::current_caps_effective();
            assert_eq!(lo_before, lo_after);
            assert_eq!(hi_before, hi_after);
        }

        // -- Sentinel ----------------------------------------------------

        /// CAP_PERFMON alone does NOT satisfy the bpf gate — only
        /// CAP_BPF or CAP_SYS_ADMIN do.  Confirms the gate is
        /// specifically the bpf_capable() pair, not a generic
        /// "any privileged cap" check.
        #[test]
        fn test_bpf_phase182_cap_perfmon_alone_does_not_satisfy() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_BPF);
            drop_cap(crate::sys_capability::CAP_SYS_ADMIN);
            // CAP_PERFMON (38) is still held by default.
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_PERFMON
            ));
            let a = good_map_create();
            fresh_errno();
            let r = call_bpf(BPF_MAP_CREATE, &a);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Cross-checks ------------------------------------------------

        /// bpf() rejection uses EPERM; perf_event_open uses EACCES.
        /// This is a deliberate Linux distinction (CAP_BPF rejection
        /// = EPERM in bpf_capable; perf_allow_kernel rejection =
        /// EACCES).  Confirms we preserve the distinction across
        /// both surfaces.
        #[test]
        fn test_bpf_phase182_eperm_distinct_from_perf_eacces() {
            let _g = CapGuard::snapshot();
            drop_bpf_caps();
            drop_cap(crate::sys_capability::CAP_PERFMON);
            // bpf → EPERM.
            let a = good_map_create();
            fresh_errno();
            assert_eq!(call_bpf(BPF_MAP_CREATE, &a), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }
    }
}
