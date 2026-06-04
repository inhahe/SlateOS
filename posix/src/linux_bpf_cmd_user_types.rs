//! `<linux/bpf.h>` — `bpf()` syscall command codes.
//!
//! `int bpf(int cmd, union bpf_attr *attr, unsigned int size)`. The
//! command is a small dense `enum bpf_cmd` selecting which
//! sub-operation (map create, program load, link create, …) the
//! kernel performs.

// ---------------------------------------------------------------------------
// `enum bpf_cmd`
// ---------------------------------------------------------------------------

pub const BPF_MAP_CREATE: u32 = 0;
pub const BPF_MAP_LOOKUP_ELEM: u32 = 1;
pub const BPF_MAP_UPDATE_ELEM: u32 = 2;
pub const BPF_MAP_DELETE_ELEM: u32 = 3;
pub const BPF_MAP_GET_NEXT_KEY: u32 = 4;
pub const BPF_PROG_LOAD: u32 = 5;
pub const BPF_OBJ_PIN: u32 = 6;
pub const BPF_OBJ_GET: u32 = 7;
pub const BPF_PROG_ATTACH: u32 = 8;
pub const BPF_PROG_DETACH: u32 = 9;
pub const BPF_PROG_TEST_RUN: u32 = 10;
pub const BPF_PROG_GET_NEXT_ID: u32 = 11;
pub const BPF_MAP_GET_NEXT_ID: u32 = 12;
pub const BPF_PROG_GET_FD_BY_ID: u32 = 13;
pub const BPF_MAP_GET_FD_BY_ID: u32 = 14;
pub const BPF_OBJ_GET_INFO_BY_FD: u32 = 15;
pub const BPF_PROG_QUERY: u32 = 16;
pub const BPF_RAW_TRACEPOINT_OPEN: u32 = 17;
pub const BPF_BTF_LOAD: u32 = 18;
pub const BPF_BTF_GET_FD_BY_ID: u32 = 19;
pub const BPF_TASK_FD_QUERY: u32 = 20;
pub const BPF_MAP_LOOKUP_AND_DELETE_ELEM: u32 = 21;
pub const BPF_MAP_FREEZE: u32 = 22;
pub const BPF_BTF_GET_NEXT_ID: u32 = 23;
pub const BPF_MAP_LOOKUP_BATCH: u32 = 24;
pub const BPF_MAP_LOOKUP_AND_DELETE_BATCH: u32 = 25;
pub const BPF_MAP_UPDATE_BATCH: u32 = 26;
pub const BPF_MAP_DELETE_BATCH: u32 = 27;
pub const BPF_LINK_CREATE: u32 = 28;
pub const BPF_LINK_UPDATE: u32 = 29;
pub const BPF_LINK_GET_FD_BY_ID: u32 = 30;
pub const BPF_LINK_GET_NEXT_ID: u32 = 31;
pub const BPF_ENABLE_STATS: u32 = 32;
pub const BPF_ITER_CREATE: u32 = 33;
pub const BPF_LINK_DETACH: u32 = 34;
pub const BPF_PROG_BIND_MAP: u32 = 35;
pub const BPF_TOKEN_CREATE: u32 = 36;

pub const __MAX_BPF_CMD: u32 = 37;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_dense_0_to_36() {
        // 37 commands 0..=36.
        assert_eq!(BPF_MAP_CREATE, 0);
        assert_eq!(BPF_TOKEN_CREATE, 36);
        assert_eq!(__MAX_BPF_CMD, 37);
    }

    #[test]
    fn test_map_family_clustered() {
        // First five commands are the original map CRUD set.
        for (i, &v) in [
            BPF_MAP_CREATE,
            BPF_MAP_LOOKUP_ELEM,
            BPF_MAP_UPDATE_ELEM,
            BPF_MAP_DELETE_ELEM,
            BPF_MAP_GET_NEXT_KEY,
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_batch_family_clustered() {
        // The four batch ops sit contiguously at 24..27.
        for v in [
            BPF_MAP_LOOKUP_BATCH,
            BPF_MAP_LOOKUP_AND_DELETE_BATCH,
            BPF_MAP_UPDATE_BATCH,
            BPF_MAP_DELETE_BATCH,
        ] {
            assert!((24..=27).contains(&v));
        }
    }

    #[test]
    fn test_link_family_clustered() {
        // BPF_LINK_* commands cluster 28..34.
        for v in [
            BPF_LINK_CREATE,
            BPF_LINK_UPDATE,
            BPF_LINK_GET_FD_BY_ID,
            BPF_LINK_GET_NEXT_ID,
            BPF_LINK_DETACH,
        ] {
            assert!((28..=34).contains(&v));
        }
    }

    #[test]
    fn test_get_fd_by_id_pair() {
        // BPF_PROG_GET_FD_BY_ID and BPF_MAP_GET_FD_BY_ID are adjacent.
        assert_eq!(
            BPF_MAP_GET_FD_BY_ID - BPF_PROG_GET_FD_BY_ID,
            1
        );
    }
}
