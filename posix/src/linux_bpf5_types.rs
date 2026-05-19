//! `<linux/bpf.h>` — Additional BPF constants (part 5).
//!
//! Supplementary BPF constants covering map update flags,
//! program query flags, and task iteration types.

// ---------------------------------------------------------------------------
// BPF map update flags
// ---------------------------------------------------------------------------

/// Create or update.
pub const BPF_ANY: u64 = 0;
/// Create only (fail if exists).
pub const BPF_NOEXIST: u64 = 1;
/// Update only (fail if doesn't exist).
pub const BPF_EXIST: u64 = 2;
/// Lock-free update.
pub const BPF_F_LOCK: u64 = 4;

// ---------------------------------------------------------------------------
// BPF map flags
// ---------------------------------------------------------------------------

/// No prealloc.
pub const BPF_F_NO_PREALLOC: u32 = 1 << 0;
/// No common LRU.
pub const BPF_F_NO_COMMON_LRU: u32 = 1 << 1;
/// NUMA node aware.
pub const BPF_F_NUMA_NODE: u32 = 1 << 2;
/// Read-only program.
pub const BPF_F_RDONLY_PROG: u32 = 1 << 3;
/// Write-only program.
pub const BPF_F_WRONLY_PROG: u32 = 1 << 4;
/// Clone support.
pub const BPF_F_CLONE: u32 = 1 << 5;
/// Map is mmapable.
pub const BPF_F_MMAPABLE: u32 = 1 << 10;
/// Preserve elems.
pub const BPF_F_PRESERVE_ELEMS: u32 = 1 << 11;
/// Inner map.
pub const BPF_F_INNER_MAP: u32 = 1 << 12;
/// Link-based.
pub const BPF_F_LINK: u32 = 1 << 13;
/// Path FD.
pub const BPF_F_PATH_FD: u32 = 1 << 14;
/// Vtype BTF FD.
pub const BPF_F_VTYPE_BTF_OBJ_FD: u32 = 1 << 15;
/// Token FD.
pub const BPF_F_TOKEN_FD: u32 = 1 << 16;
/// Segv on fault.
pub const BPF_F_SEGV_ON_FAULT: u32 = 1 << 17;
/// No user conv.
pub const BPF_F_NO_USER_CONV: u32 = 1 << 18;

// ---------------------------------------------------------------------------
// BPF object access types
// ---------------------------------------------------------------------------

/// Read.
pub const BPF_OBJ_READ: u32 = 1 << 0;
/// Write.
pub const BPF_OBJ_WRITE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_map_flags_no_overlap() {
        let flags = [
            BPF_F_NO_PREALLOC, BPF_F_NO_COMMON_LRU,
            BPF_F_NUMA_NODE, BPF_F_RDONLY_PROG,
            BPF_F_WRONLY_PROG, BPF_F_CLONE,
            BPF_F_MMAPABLE, BPF_F_PRESERVE_ELEMS,
            BPF_F_INNER_MAP, BPF_F_LINK,
            BPF_F_PATH_FD, BPF_F_VTYPE_BTF_OBJ_FD,
            BPF_F_TOKEN_FD, BPF_F_SEGV_ON_FAULT,
            BPF_F_NO_USER_CONV,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_obj_access_no_overlap() {
        assert_eq!(BPF_OBJ_READ & BPF_OBJ_WRITE, 0);
    }
}
