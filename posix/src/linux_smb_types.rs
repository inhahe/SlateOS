//! `<linux/smb.h>` — SMB/CIFS protocol constants.
//!
//! SMB (Server Message Block) is a network file sharing protocol,
//! primarily used by Windows. Linux implements it via the CIFS/ksmbd
//! kernel modules. SMB2/3 is the modern version with better performance,
//! encryption, and multi-channel support.

// ---------------------------------------------------------------------------
// SMB command codes (SMB1)
// ---------------------------------------------------------------------------

/// Negotiate protocol.
pub const SMB_COM_NEGOTIATE: u8 = 0x72;
/// Session setup.
pub const SMB_COM_SESSION_SETUP: u8 = 0x73;
/// Tree connect.
pub const SMB_COM_TREE_CONNECT: u8 = 0x75;
/// Tree disconnect.
pub const SMB_COM_TREE_DISCONNECT: u8 = 0x71;
/// Create file.
pub const SMB_COM_CREATE: u8 = 0xA2;
/// Close file.
pub const SMB_COM_CLOSE: u8 = 0x04;
/// Read.
pub const SMB_COM_READ: u8 = 0x2E;
/// Write.
pub const SMB_COM_WRITE: u8 = 0x2F;
/// Logoff.
pub const SMB_COM_LOGOFF: u8 = 0x74;

// ---------------------------------------------------------------------------
// SMB2 command codes
// ---------------------------------------------------------------------------

/// SMB2 Negotiate.
pub const SMB2_NEGOTIATE: u16 = 0x0000;
/// SMB2 Session Setup.
pub const SMB2_SESSION_SETUP: u16 = 0x0001;
/// SMB2 Logoff.
pub const SMB2_LOGOFF: u16 = 0x0002;
/// SMB2 Tree Connect.
pub const SMB2_TREE_CONNECT: u16 = 0x0003;
/// SMB2 Tree Disconnect.
pub const SMB2_TREE_DISCONNECT: u16 = 0x0004;
/// SMB2 Create.
pub const SMB2_CREATE: u16 = 0x0005;
/// SMB2 Close.
pub const SMB2_CLOSE: u16 = 0x0006;
/// SMB2 Flush.
pub const SMB2_FLUSH: u16 = 0x0007;
/// SMB2 Read.
pub const SMB2_READ: u16 = 0x0008;
/// SMB2 Write.
pub const SMB2_WRITE: u16 = 0x0009;
/// SMB2 Lock.
pub const SMB2_LOCK: u16 = 0x000A;
/// SMB2 Ioctl.
pub const SMB2_IOCTL: u16 = 0x000B;
/// SMB2 Cancel.
pub const SMB2_CANCEL: u16 = 0x000C;
/// SMB2 Echo.
pub const SMB2_ECHO: u16 = 0x000D;
/// SMB2 Query Directory.
pub const SMB2_QUERY_DIRECTORY: u16 = 0x000E;
/// SMB2 Change Notify.
pub const SMB2_CHANGE_NOTIFY: u16 = 0x000F;
/// SMB2 Query Info.
pub const SMB2_QUERY_INFO: u16 = 0x0010;
/// SMB2 Set Info.
pub const SMB2_SET_INFO: u16 = 0x0011;
/// SMB2 Oplock Break.
pub const SMB2_OPLOCK_BREAK: u16 = 0x0012;

// ---------------------------------------------------------------------------
// SMB dialect versions
// ---------------------------------------------------------------------------

/// SMB 2.0.2.
pub const SMB2_DIALECT_0202: u16 = 0x0202;
/// SMB 2.1.
pub const SMB2_DIALECT_0210: u16 = 0x0210;
/// SMB 3.0.
pub const SMB2_DIALECT_0300: u16 = 0x0300;
/// SMB 3.0.2.
pub const SMB2_DIALECT_0302: u16 = 0x0302;
/// SMB 3.1.1.
pub const SMB2_DIALECT_0311: u16 = 0x0311;

// ---------------------------------------------------------------------------
// SMB share types
// ---------------------------------------------------------------------------

/// Disk share.
pub const SMB_SHARE_DISK: u8 = 0x01;
/// Printer share.
pub const SMB_SHARE_PRINTER: u8 = 0x02;
/// Named pipe share (IPC$).
pub const SMB_SHARE_PIPE: u8 = 0x03;

// ---------------------------------------------------------------------------
// SMB default port
// ---------------------------------------------------------------------------

/// SMB over TCP direct (port 445).
pub const SMB_PORT: u16 = 445;
/// NetBIOS session service (port 139).
pub const SMB_NETBIOS_PORT: u16 = 139;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smb1_commands_distinct() {
        let cmds = [
            SMB_COM_NEGOTIATE, SMB_COM_SESSION_SETUP, SMB_COM_TREE_CONNECT,
            SMB_COM_TREE_DISCONNECT, SMB_COM_CREATE, SMB_COM_CLOSE,
            SMB_COM_READ, SMB_COM_WRITE, SMB_COM_LOGOFF,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_smb2_commands_distinct() {
        let cmds = [
            SMB2_NEGOTIATE, SMB2_SESSION_SETUP, SMB2_LOGOFF,
            SMB2_TREE_CONNECT, SMB2_TREE_DISCONNECT, SMB2_CREATE,
            SMB2_CLOSE, SMB2_FLUSH, SMB2_READ, SMB2_WRITE,
            SMB2_LOCK, SMB2_IOCTL, SMB2_CANCEL, SMB2_ECHO,
            SMB2_QUERY_DIRECTORY, SMB2_CHANGE_NOTIFY,
            SMB2_QUERY_INFO, SMB2_SET_INFO, SMB2_OPLOCK_BREAK,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_dialects_distinct() {
        let dialects = [
            SMB2_DIALECT_0202, SMB2_DIALECT_0210, SMB2_DIALECT_0300,
            SMB2_DIALECT_0302, SMB2_DIALECT_0311,
        ];
        for i in 0..dialects.len() {
            for j in (i + 1)..dialects.len() {
                assert_ne!(dialects[i], dialects[j]);
            }
        }
    }

    #[test]
    fn test_share_types_distinct() {
        let types = [SMB_SHARE_DISK, SMB_SHARE_PRINTER, SMB_SHARE_PIPE];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ports() {
        assert_eq!(SMB_PORT, 445);
        assert_eq!(SMB_NETBIOS_PORT, 139);
        assert_ne!(SMB_PORT, SMB_NETBIOS_PORT);
    }
}
