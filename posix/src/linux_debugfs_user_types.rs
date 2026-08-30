//! `<linux/debugfs.h>` — debugfs filesystem user-facing constants.
//!
//! debugfs is a virtual filesystem at `/sys/kernel/debug` used by kernel
//! drivers to expose tunables, counters, and traces. It is intended for
//! kernel developers — not a stable ABI. Mounted with no options other
//! than `mode=` / `uid=` / `gid=`.

// ---------------------------------------------------------------------------
// Filesystem identity
// ---------------------------------------------------------------------------

pub const DEBUGFS_NAME: &str = "debugfs";
pub const DEBUGFS_MOUNT_POINT: &str = "/sys/kernel/debug";
/// statfs() f_type magic.
pub const DEBUGFS_MAGIC: u32 = 0x64626720;

// ---------------------------------------------------------------------------
// Mount options
// ---------------------------------------------------------------------------

pub const DEBUGFS_OPT_MODE: &str = "mode";
pub const DEBUGFS_OPT_UID: &str = "uid";
pub const DEBUGFS_OPT_GID: &str = "gid";
pub const DEBUGFS_OPT_SOURCE: &str = "source";

// ---------------------------------------------------------------------------
// Default permissions for debugfs entries
// ---------------------------------------------------------------------------

/// debugfs entries default to 0600 (root only).
pub const DEBUGFS_DEFAULT_MODE: u32 = 0o600;
/// Read-only entries default to 0400.
pub const DEBUGFS_RO_MODE: u32 = 0o400;
/// Write-only entries default to 0200.
pub const DEBUGFS_WO_MODE: u32 = 0o200;
/// Default dir mode 0700.
pub const DEBUGFS_DIR_MODE: u32 = 0o700;

// ---------------------------------------------------------------------------
// Commonly-used file names in debugfs trees
// ---------------------------------------------------------------------------

pub const DEBUGFS_FILE_TRACE: &str = "trace";
pub const DEBUGFS_FILE_TRACING_ON: &str = "tracing_on";
pub const DEBUGFS_FILE_CURRENT_TRACER: &str = "current_tracer";
pub const DEBUGFS_FILE_AVAILABLE_TRACERS: &str = "available_tracers";
pub const DEBUGFS_FILE_TRACE_PIPE: &str = "trace_pipe";
pub const DEBUGFS_FILE_TRACE_CLOCK: &str = "trace_clock";

// ---------------------------------------------------------------------------
// debugfs subdirs
// ---------------------------------------------------------------------------

pub const DEBUGFS_DIR_TRACING: &str = "tracing";
pub const DEBUGFS_DIR_DYNAMIC_DEBUG: &str = "dynamic_debug";
pub const DEBUGFS_DIR_SCHED: &str = "sched";
pub const DEBUGFS_DIR_BLOCK: &str = "block";
pub const DEBUGFS_DIR_BLUETOOTH: &str = "bluetooth";

// ---------------------------------------------------------------------------
// File operation type tags (used internally for blob/u32/x32 wrappers)
// ---------------------------------------------------------------------------

pub const DEBUGFS_TYPE_U8: u8 = 0;
pub const DEBUGFS_TYPE_U16: u8 = 1;
pub const DEBUGFS_TYPE_U32: u8 = 2;
pub const DEBUGFS_TYPE_U64: u8 = 3;
pub const DEBUGFS_TYPE_X8: u8 = 4;
pub const DEBUGFS_TYPE_X16: u8 = 5;
pub const DEBUGFS_TYPE_X32: u8 = 6;
pub const DEBUGFS_TYPE_X64: u8 = 7;
pub const DEBUGFS_TYPE_SIZE_T: u8 = 8;
pub const DEBUGFS_TYPE_ATOMIC_T: u8 = 9;
pub const DEBUGFS_TYPE_BOOL: u8 = 10;
pub const DEBUGFS_TYPE_BLOB: u8 = 11;
pub const DEBUGFS_TYPE_REG32: u8 = 12;
pub const DEBUGFS_TYPE_ULONG: u8 = 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_name_and_magic() {
        assert_eq!(DEBUGFS_NAME, "debugfs");
        // Magic is ASCII "dbg " (0x64 0x62 0x67 0x20).
        assert_eq!(DEBUGFS_MAGIC, 0x64626720);
        let bytes = DEBUGFS_MAGIC.to_be_bytes();
        assert_eq!(&bytes, b"dbg ");
    }

    #[test]
    fn test_mount_point_under_sys_kernel() {
        assert_eq!(DEBUGFS_MOUNT_POINT, "/sys/kernel/debug");
        assert!(DEBUGFS_MOUNT_POINT.starts_with("/sys/kernel/"));
    }

    #[test]
    fn test_modes_are_root_only() {
        // No "other" or "group" bits set.
        for m in [
            DEBUGFS_DEFAULT_MODE,
            DEBUGFS_RO_MODE,
            DEBUGFS_WO_MODE,
            DEBUGFS_DIR_MODE,
        ] {
            assert_eq!(m & 0o077, 0, "mode {m:o} leaks to non-owner");
        }
    }

    #[test]
    fn test_ro_wo_combine_to_rw() {
        assert_eq!(DEBUGFS_RO_MODE | DEBUGFS_WO_MODE, DEBUGFS_DEFAULT_MODE);
    }

    #[test]
    fn test_dir_mode_has_exec_bit() {
        // Owner needs +x to traverse the directory.
        assert_eq!(DEBUGFS_DIR_MODE & 0o100, 0o100);
    }

    #[test]
    fn test_type_tags_dense_0_to_13() {
        let t = [
            DEBUGFS_TYPE_U8,
            DEBUGFS_TYPE_U16,
            DEBUGFS_TYPE_U32,
            DEBUGFS_TYPE_U64,
            DEBUGFS_TYPE_X8,
            DEBUGFS_TYPE_X16,
            DEBUGFS_TYPE_X32,
            DEBUGFS_TYPE_X64,
            DEBUGFS_TYPE_SIZE_T,
            DEBUGFS_TYPE_ATOMIC_T,
            DEBUGFS_TYPE_BOOL,
            DEBUGFS_TYPE_BLOB,
            DEBUGFS_TYPE_REG32,
            DEBUGFS_TYPE_ULONG,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_u_and_x_pairs_offset_by_4() {
        // U-series and X-series sizes are parallel: U64 + 4 == X64.
        assert_eq!(DEBUGFS_TYPE_X8, DEBUGFS_TYPE_U8 + 4);
        assert_eq!(DEBUGFS_TYPE_X16, DEBUGFS_TYPE_U16 + 4);
        assert_eq!(DEBUGFS_TYPE_X32, DEBUGFS_TYPE_U32 + 4);
        assert_eq!(DEBUGFS_TYPE_X64, DEBUGFS_TYPE_U64 + 4);
    }

    #[test]
    fn test_mount_options_distinct() {
        let o = [
            DEBUGFS_OPT_MODE,
            DEBUGFS_OPT_UID,
            DEBUGFS_OPT_GID,
            DEBUGFS_OPT_SOURCE,
        ];
        for (i, &x) in o.iter().enumerate() {
            for &y in &o[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_trace_file_names_distinct_lowercase() {
        for n in [
            DEBUGFS_FILE_TRACE,
            DEBUGFS_FILE_TRACING_ON,
            DEBUGFS_FILE_CURRENT_TRACER,
            DEBUGFS_FILE_AVAILABLE_TRACERS,
            DEBUGFS_FILE_TRACE_PIPE,
            DEBUGFS_FILE_TRACE_CLOCK,
        ] {
            assert!(n.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }
}
