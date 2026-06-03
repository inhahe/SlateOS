//! `<linux/cachefiles.h>` — CacheFiles userspace daemon protocol.
//!
//! The CacheFiles back-end (used by fscache for NFS, AFS, 9p,
//! Ceph) accepts commands from a userspace daemon (`cachefilesd`)
//! via `/dev/cachefiles`. The protocol exchanges textual commands
//! and 64-bit operation tags. Constants below cover the
//! command-name tokens and ioctl numbers used by cachefilesd.

// ---------------------------------------------------------------------------
// /dev/cachefiles text commands (cachefilesd writes ASCII)
// ---------------------------------------------------------------------------

/// "bind" — finish setup and start the cache.
pub const CACHEFILES_CMD_BIND: &str = "bind";
/// "frun" — set the free-space resume threshold (%).
pub const CACHEFILES_CMD_FRUN: &str = "frun";
/// "fcull" — set the free-space cull threshold (%).
pub const CACHEFILES_CMD_FCULL: &str = "fcull";
/// "fstop" — set the free-space stop threshold (%).
pub const CACHEFILES_CMD_FSTOP: &str = "fstop";
/// "brun" — set the blocks resume threshold (%).
pub const CACHEFILES_CMD_BRUN: &str = "brun";
/// "bcull" — set the blocks cull threshold (%).
pub const CACHEFILES_CMD_BCULL: &str = "bcull";
/// "bstop" — set the blocks stop threshold (%).
pub const CACHEFILES_CMD_BSTOP: &str = "bstop";
/// "cull" — cull the next victim object.
pub const CACHEFILES_CMD_CULL: &str = "cull";
/// "dir" — set the cache directory path.
pub const CACHEFILES_CMD_DIR: &str = "dir";
/// "inuse" — query whether a file is in use.
pub const CACHEFILES_CMD_INUSE: &str = "inuse";
/// "tag" — set a textual cache tag.
pub const CACHEFILES_CMD_TAG: &str = "tag";

// ---------------------------------------------------------------------------
// Threshold percentage ranges
// ---------------------------------------------------------------------------

/// Default frun threshold (%).
pub const CACHEFILES_FRUN_DEFAULT: u32 = 7;
/// Default fcull threshold (%).
pub const CACHEFILES_FCULL_DEFAULT: u32 = 5;
/// Default fstop threshold (%).
pub const CACHEFILES_FSTOP_DEFAULT: u32 = 1;

// ---------------------------------------------------------------------------
// Daemon-controlled ioctls (modern on-demand mode)
// ---------------------------------------------------------------------------

/// Magic byte for cachefiles ioctls ('0xc1').
pub const CACHEFILES_IOC_MAGIC: u8 = 0xc1;
/// `CACHEFILES_IOC_READ_COMPLETE` — daemon signals a read finished.
pub const CACHEFILES_IOC_READ_COMPLETE: u32 = 0x4008_c101;
/// `CACHEFILES_IOC_OPEN` — daemon opens a backing file.
pub const CACHEFILES_IOC_OPEN: u32 = 0x4010_c102;
/// `CACHEFILES_IOC_CLOSE` — daemon closes a backing file.
pub const CACHEFILES_IOC_CLOSE: u32 = 0x4008_c103;
/// `CACHEFILES_IOC_RESTORE` — daemon restores a request.
pub const CACHEFILES_IOC_RESTORE: u32 = 0x0000_c104;

// ---------------------------------------------------------------------------
// On-demand request opcodes (struct cachefiles_msg.opcode)
// ---------------------------------------------------------------------------

/// Open an on-demand backing file.
pub const CACHEFILES_OP_OPEN: u32 = 0;
/// Close an on-demand backing file.
pub const CACHEFILES_OP_CLOSE: u32 = 1;
/// Read into an on-demand backing file.
pub const CACHEFILES_OP_READ: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_tokens_distinct_and_lowercase() {
        let c = [
            CACHEFILES_CMD_BIND,
            CACHEFILES_CMD_FRUN,
            CACHEFILES_CMD_FCULL,
            CACHEFILES_CMD_FSTOP,
            CACHEFILES_CMD_BRUN,
            CACHEFILES_CMD_BCULL,
            CACHEFILES_CMD_BSTOP,
            CACHEFILES_CMD_CULL,
            CACHEFILES_CMD_DIR,
            CACHEFILES_CMD_INUSE,
            CACHEFILES_CMD_TAG,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
            // cachefilesd writes lowercase command names.
            assert!(c[i].chars().all(|x| x.is_ascii_lowercase()));
        }
    }

    #[test]
    fn test_default_thresholds_ordered() {
        // For the cache logic to work, stop < cull < run.
        assert!(CACHEFILES_FSTOP_DEFAULT < CACHEFILES_FCULL_DEFAULT);
        assert!(CACHEFILES_FCULL_DEFAULT < CACHEFILES_FRUN_DEFAULT);
        // Thresholds are percentages.
        assert!(CACHEFILES_FRUN_DEFAULT <= 100);
    }

    #[test]
    fn test_ioctls_distinct_and_use_magic() {
        let ops = [
            CACHEFILES_IOC_READ_COMPLETE,
            CACHEFILES_IOC_OPEN,
            CACHEFILES_IOC_CLOSE,
            CACHEFILES_IOC_RESTORE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte 0xc1.
            assert_eq!((ops[i] >> 8) & 0xff, CACHEFILES_IOC_MAGIC as u32);
        }
    }

    #[test]
    fn test_opcodes_dense() {
        let o = [CACHEFILES_OP_OPEN, CACHEFILES_OP_CLOSE, CACHEFILES_OP_READ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}
