//! `<linux/bpf.h>` — eBPF syscall command constants.
//!
//! The `bpf()` syscall uses a command argument to select operations
//! like creating maps, loading programs, attaching to hooks, and
//! querying BPF objects. These constants enumerate the available
//! commands.

// ---------------------------------------------------------------------------
// bpf() syscall commands (BPF_CMD_*)
// ---------------------------------------------------------------------------

/// Create a new BPF map.
pub const BPF_MAP_CREATE: u32 = 0;
/// Look up an element in a map.
pub const BPF_MAP_LOOKUP_ELEM: u32 = 1;
/// Create or update a map element.
pub const BPF_MAP_UPDATE_ELEM: u32 = 2;
/// Delete a map element.
pub const BPF_MAP_DELETE_ELEM: u32 = 3;
/// Iterate to next map element.
pub const BPF_MAP_GET_NEXT_KEY: u32 = 4;
/// Load a BPF program.
pub const BPF_PROG_LOAD: u32 = 5;
/// Attach a BPF program to a hook.
pub const BPF_PROG_ATTACH: u32 = 8;
/// Detach a BPF program from a hook.
pub const BPF_PROG_DETACH: u32 = 9;
/// Test-run a BPF program.
pub const BPF_PROG_TEST_RUN: u32 = 10;
/// Get next BPF program ID.
pub const BPF_PROG_GET_NEXT_ID: u32 = 11;
/// Get next BPF map ID.
pub const BPF_MAP_GET_NEXT_ID: u32 = 12;
/// Get BPF program fd by ID.
pub const BPF_PROG_GET_FD_BY_ID: u32 = 13;
/// Get BPF map fd by ID.
pub const BPF_MAP_GET_FD_BY_ID: u32 = 14;
/// Get info about a BPF object.
pub const BPF_OBJ_GET_INFO_BY_FD: u32 = 15;
/// Query attached BPF programs.
pub const BPF_PROG_QUERY: u32 = 16;
/// Batch map lookup.
pub const BPF_MAP_LOOKUP_BATCH: u32 = 24;
/// Batch map lookup and delete.
pub const BPF_MAP_LOOKUP_AND_DELETE_BATCH: u32 = 25;
/// Batch map update.
pub const BPF_MAP_UPDATE_BATCH: u32 = 26;
/// Batch map delete.
pub const BPF_MAP_DELETE_BATCH: u32 = 27;
/// Create a BPF link.
pub const BPF_LINK_CREATE: u32 = 28;
/// Update a BPF link.
pub const BPF_LINK_UPDATE: u32 = 29;
/// Get next BPF link ID.
pub const BPF_LINK_GET_NEXT_ID: u32 = 31;
/// Get BPF link fd by ID.
pub const BPF_LINK_GET_FD_BY_ID: u32 = 32;
/// Pin a BPF object to bpffs.
pub const BPF_OBJ_PIN: u32 = 6;
/// Get a pinned BPF object.
pub const BPF_OBJ_GET: u32 = 7;
/// Enable BPF stats collection.
pub const BPF_ENABLE_STATS: u32 = 32;
/// Detach a BPF link.
pub const BPF_LINK_DETACH: u32 = 34;
/// Freeze a BPF map (make read-only).
pub const BPF_MAP_FREEZE: u32 = 22;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_map_ops_sequential() {
        assert_eq!(BPF_MAP_CREATE, 0);
        assert_eq!(BPF_MAP_LOOKUP_ELEM, 1);
        assert_eq!(BPF_MAP_UPDATE_ELEM, 2);
        assert_eq!(BPF_MAP_DELETE_ELEM, 3);
        assert_eq!(BPF_MAP_GET_NEXT_KEY, 4);
    }

    #[test]
    fn test_prog_load() {
        assert_eq!(BPF_PROG_LOAD, 5);
    }

    #[test]
    fn test_obj_pin_get() {
        assert_eq!(BPF_OBJ_PIN, 6);
        assert_eq!(BPF_OBJ_GET, 7);
    }

    #[test]
    fn test_attach_detach() {
        assert_eq!(BPF_PROG_ATTACH, 8);
        assert_eq!(BPF_PROG_DETACH, 9);
    }

    #[test]
    fn test_link_ops() {
        assert_eq!(BPF_LINK_CREATE, 28);
        assert_eq!(BPF_LINK_UPDATE, 29);
    }

    #[test]
    fn test_batch_ops_distinct() {
        let batch = [
            BPF_MAP_LOOKUP_BATCH, BPF_MAP_LOOKUP_AND_DELETE_BATCH,
            BPF_MAP_UPDATE_BATCH, BPF_MAP_DELETE_BATCH,
        ];
        for i in 0..batch.len() {
            for j in (i + 1)..batch.len() {
                assert_ne!(batch[i], batch[j]);
            }
        }
    }
}
