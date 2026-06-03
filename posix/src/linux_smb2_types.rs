//! `<linux/cifs/cifs_mount.h>` — SMB/CIFS protocol constants.
//!
//! SMB (Server Message Block) / CIFS constants covering
//! protocol versions, command codes, access masks,
//! and share types.

// ---------------------------------------------------------------------------
// Protocol versions
// ---------------------------------------------------------------------------

/// SMB 1.0.
pub const SMB_PROTOCOL_SMB1: u32 = 0x0100;
/// SMB 2.0.
pub const SMB_PROTOCOL_SMB2_02: u32 = 0x0202;
/// SMB 2.1.
pub const SMB_PROTOCOL_SMB2_10: u32 = 0x0210;
/// SMB 3.0.
pub const SMB_PROTOCOL_SMB3_00: u32 = 0x0300;
/// SMB 3.0.2.
pub const SMB_PROTOCOL_SMB3_02: u32 = 0x0302;
/// SMB 3.1.1.
pub const SMB_PROTOCOL_SMB3_11: u32 = 0x0311;

// ---------------------------------------------------------------------------
// SMB2 command codes
// ---------------------------------------------------------------------------

/// Negotiate.
pub const SMB2_NEGOTIATE: u16 = 0x0000;
/// Session setup.
pub const SMB2_SESSION_SETUP: u16 = 0x0001;
/// Session logoff.
pub const SMB2_LOGOFF: u16 = 0x0002;
/// Tree connect.
pub const SMB2_TREE_CONNECT: u16 = 0x0003;
/// Tree disconnect.
pub const SMB2_TREE_DISCONNECT: u16 = 0x0004;
/// Create.
pub const SMB2_CREATE: u16 = 0x0005;
/// Close.
pub const SMB2_CLOSE: u16 = 0x0006;
/// Flush.
pub const SMB2_FLUSH: u16 = 0x0007;
/// Read.
pub const SMB2_READ: u16 = 0x0008;
/// Write.
pub const SMB2_WRITE: u16 = 0x0009;
/// Lock.
pub const SMB2_LOCK: u16 = 0x000A;
/// IOCTL.
pub const SMB2_IOCTL: u16 = 0x000B;
/// Cancel.
pub const SMB2_CANCEL: u16 = 0x000C;
/// Echo.
pub const SMB2_ECHO: u16 = 0x000D;
/// Query directory.
pub const SMB2_QUERY_DIRECTORY: u16 = 0x000E;
/// Change notify.
pub const SMB2_CHANGE_NOTIFY: u16 = 0x000F;
/// Query info.
pub const SMB2_QUERY_INFO: u16 = 0x0010;
/// Set info.
pub const SMB2_SET_INFO: u16 = 0x0011;
/// Oplock break.
pub const SMB2_OPLOCK_BREAK: u16 = 0x0012;

// ---------------------------------------------------------------------------
// Access mask bits
// ---------------------------------------------------------------------------

/// Read data / list directory.
pub const SMB2_FILE_READ_DATA: u32 = 0x00000001;
/// Write data / add file.
pub const SMB2_FILE_WRITE_DATA: u32 = 0x00000002;
/// Append data / add subdir.
pub const SMB2_FILE_APPEND_DATA: u32 = 0x00000004;
/// Read EA.
pub const SMB2_FILE_READ_EA: u32 = 0x00000008;
/// Write EA.
pub const SMB2_FILE_WRITE_EA: u32 = 0x00000010;
/// Execute / traverse.
pub const SMB2_FILE_EXECUTE: u32 = 0x00000020;
/// Delete child.
pub const SMB2_FILE_DELETE_CHILD: u32 = 0x00000040;
/// Read attributes.
pub const SMB2_FILE_READ_ATTRIBUTES: u32 = 0x00000080;
/// Write attributes.
pub const SMB2_FILE_WRITE_ATTRIBUTES: u32 = 0x00000100;
/// Delete.
pub const SMB2_DELETE: u32 = 0x00010000;
/// Read control.
pub const SMB2_READ_CONTROL: u32 = 0x00020000;
/// Write DAC.
pub const SMB2_WRITE_DAC: u32 = 0x00040000;
/// Write owner.
pub const SMB2_WRITE_OWNER: u32 = 0x00080000;

// ---------------------------------------------------------------------------
// Share types
// ---------------------------------------------------------------------------

/// Disk share.
pub const SMB2_SHARE_TYPE_DISK: u8 = 0x01;
/// Pipe share.
pub const SMB2_SHARE_TYPE_PIPE: u8 = 0x02;
/// Print share.
pub const SMB2_SHARE_TYPE_PRINT: u8 = 0x03;

// ---------------------------------------------------------------------------
// Oplock levels
// ---------------------------------------------------------------------------

/// No oplock.
pub const SMB2_OPLOCK_LEVEL_NONE: u8 = 0x00;
/// Level II oplock.
pub const SMB2_OPLOCK_LEVEL_II: u8 = 0x01;
/// Exclusive oplock.
pub const SMB2_OPLOCK_LEVEL_EXCLUSIVE: u8 = 0x08;
/// Batch oplock.
pub const SMB2_OPLOCK_LEVEL_BATCH: u8 = 0x09;
/// Lease oplock.
pub const SMB2_OPLOCK_LEVEL_LEASE: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_versions_distinct() {
        let versions = [
            SMB_PROTOCOL_SMB1,
            SMB_PROTOCOL_SMB2_02,
            SMB_PROTOCOL_SMB2_10,
            SMB_PROTOCOL_SMB3_00,
            SMB_PROTOCOL_SMB3_02,
            SMB_PROTOCOL_SMB3_11,
        ];
        for i in 0..versions.len() {
            for j in (i + 1)..versions.len() {
                assert_ne!(versions[i], versions[j]);
            }
        }
    }

    #[test]
    fn test_commands_sequential() {
        assert_eq!(SMB2_NEGOTIATE, 0);
        assert_eq!(SMB2_SESSION_SETUP, 1);
        assert_eq!(SMB2_OPLOCK_BREAK, 0x0012);
    }

    #[test]
    fn test_commands_distinct() {
        let cmds: [u16; 19] = [
            SMB2_NEGOTIATE,
            SMB2_SESSION_SETUP,
            SMB2_LOGOFF,
            SMB2_TREE_CONNECT,
            SMB2_TREE_DISCONNECT,
            SMB2_CREATE,
            SMB2_CLOSE,
            SMB2_FLUSH,
            SMB2_READ,
            SMB2_WRITE,
            SMB2_LOCK,
            SMB2_IOCTL,
            SMB2_CANCEL,
            SMB2_ECHO,
            SMB2_QUERY_DIRECTORY,
            SMB2_CHANGE_NOTIFY,
            SMB2_QUERY_INFO,
            SMB2_SET_INFO,
            SMB2_OPLOCK_BREAK,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_access_mask_power_of_two() {
        let bits = [
            SMB2_FILE_READ_DATA,
            SMB2_FILE_WRITE_DATA,
            SMB2_FILE_APPEND_DATA,
            SMB2_FILE_READ_EA,
            SMB2_FILE_WRITE_EA,
            SMB2_FILE_EXECUTE,
            SMB2_FILE_DELETE_CHILD,
            SMB2_FILE_READ_ATTRIBUTES,
            SMB2_FILE_WRITE_ATTRIBUTES,
            SMB2_DELETE,
            SMB2_READ_CONTROL,
            SMB2_WRITE_DAC,
            SMB2_WRITE_OWNER,
        ];
        for b in &bits {
            assert!(b.is_power_of_two(), "0x{:08x} not power of two", b);
        }
    }

    #[test]
    fn test_share_types_distinct() {
        let types: [u8; 3] = [
            SMB2_SHARE_TYPE_DISK,
            SMB2_SHARE_TYPE_PIPE,
            SMB2_SHARE_TYPE_PRINT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_oplock_levels_distinct() {
        let levels: [u8; 5] = [
            SMB2_OPLOCK_LEVEL_NONE,
            SMB2_OPLOCK_LEVEL_II,
            SMB2_OPLOCK_LEVEL_EXCLUSIVE,
            SMB2_OPLOCK_LEVEL_BATCH,
            SMB2_OPLOCK_LEVEL_LEASE,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }
}
