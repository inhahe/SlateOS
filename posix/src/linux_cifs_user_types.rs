//! `<linux/cifs/cifs.h>` — SMB command codes and NT status codes.
//!
//! The SMB protocol uses single-byte command codes in the SMB header
//! (legacy SMB1) and 16-bit command codes in SMB2/3 headers. NT_STATUS
//! codes are 32-bit return values matching Windows error codes.

// ---------------------------------------------------------------------------
// SMB1 command codes (single byte in SMB header)
// ---------------------------------------------------------------------------

pub const SMB_COM_CREATE_DIRECTORY: u8 = 0x00;
pub const SMB_COM_DELETE_DIRECTORY: u8 = 0x01;
pub const SMB_COM_OPEN: u8 = 0x02;
pub const SMB_COM_CLOSE: u8 = 0x04;
pub const SMB_COM_DELETE: u8 = 0x06;
pub const SMB_COM_RENAME: u8 = 0x07;
pub const SMB_COM_READ: u8 = 0x0A;
pub const SMB_COM_WRITE: u8 = 0x0B;
pub const SMB_COM_LOCK_BYTE_RANGE: u8 = 0x0C;
pub const SMB_COM_UNLOCK_BYTE_RANGE: u8 = 0x0D;
pub const SMB_COM_TREE_CONNECT: u8 = 0x70;
pub const SMB_COM_TREE_DISCONNECT: u8 = 0x71;
pub const SMB_COM_NEGOTIATE: u8 = 0x72;
pub const SMB_COM_SESSION_SETUP_ANDX: u8 = 0x73;
pub const SMB_COM_LOGOFF_ANDX: u8 = 0x74;

// ---------------------------------------------------------------------------
// SMB2/3 command codes (16-bit)
// ---------------------------------------------------------------------------

pub const SMB2_NEGOTIATE: u16 = 0x0000;
pub const SMB2_SESSION_SETUP: u16 = 0x0001;
pub const SMB2_LOGOFF: u16 = 0x0002;
pub const SMB2_TREE_CONNECT: u16 = 0x0003;
pub const SMB2_TREE_DISCONNECT: u16 = 0x0004;
pub const SMB2_CREATE: u16 = 0x0005;
pub const SMB2_CLOSE: u16 = 0x0006;
pub const SMB2_FLUSH: u16 = 0x0007;
pub const SMB2_READ: u16 = 0x0008;
pub const SMB2_WRITE: u16 = 0x0009;
pub const SMB2_LOCK: u16 = 0x000A;
pub const SMB2_IOCTL: u16 = 0x000B;
pub const SMB2_CANCEL: u16 = 0x000C;
pub const SMB2_ECHO: u16 = 0x000D;
pub const SMB2_QUERY_DIRECTORY: u16 = 0x000E;
pub const SMB2_CHANGE_NOTIFY: u16 = 0x000F;
pub const SMB2_QUERY_INFO: u16 = 0x0010;
pub const SMB2_SET_INFO: u16 = 0x0011;
pub const SMB2_OPLOCK_BREAK: u16 = 0x0012;

// ---------------------------------------------------------------------------
// NT_STATUS — common return codes (Windows-style 32-bit)
// ---------------------------------------------------------------------------

pub const NT_STATUS_SUCCESS: u32 = 0x0000_0000;
pub const NT_STATUS_PENDING: u32 = 0x0000_0103;
pub const NT_STATUS_BUFFER_OVERFLOW: u32 = 0x8000_0005;
pub const NT_STATUS_NO_MORE_FILES: u32 = 0x8000_0006;
pub const NT_STATUS_UNSUCCESSFUL: u32 = 0xC000_0001;
pub const NT_STATUS_INVALID_HANDLE: u32 = 0xC000_0008;
pub const NT_STATUS_ACCESS_DENIED: u32 = 0xC000_0022;
pub const NT_STATUS_LOGON_FAILURE: u32 = 0xC000_006D;
pub const NT_STATUS_OBJECT_NAME_NOT_FOUND: u32 = 0xC000_0034;
pub const NT_STATUS_OBJECT_NAME_COLLISION: u32 = 0xC000_0035;

// ---------------------------------------------------------------------------
// NT_STATUS severity (top 2 bits)
// ---------------------------------------------------------------------------

/// Severity mask covers bits 30..31 of an NTSTATUS code.
pub const NT_STATUS_SEVERITY_MASK: u32 = 0xC000_0000;
pub const NT_STATUS_SEVERITY_SUCCESS: u32 = 0x0000_0000;
pub const NT_STATUS_SEVERITY_INFO: u32 = 0x4000_0000;
pub const NT_STATUS_SEVERITY_WARNING: u32 = 0x8000_0000;
pub const NT_STATUS_SEVERITY_ERROR: u32 = 0xC000_0000;

// ---------------------------------------------------------------------------
// SMB header magic
// ---------------------------------------------------------------------------

/// SMB1 header signature: 0xFF 'S' 'M' 'B'.
pub const SMB_HEADER_MAGIC: [u8; 4] = [0xFF, b'S', b'M', b'B'];
/// SMB2/3 header signature: 0xFE 'S' 'M' 'B'.
pub const SMB2_HEADER_MAGIC: [u8; 4] = [0xFE, b'S', b'M', b'B'];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smb1_commands_distinct() {
        let c = [
            SMB_COM_CREATE_DIRECTORY,
            SMB_COM_DELETE_DIRECTORY,
            SMB_COM_OPEN,
            SMB_COM_CLOSE,
            SMB_COM_DELETE,
            SMB_COM_RENAME,
            SMB_COM_READ,
            SMB_COM_WRITE,
            SMB_COM_LOCK_BYTE_RANGE,
            SMB_COM_UNLOCK_BYTE_RANGE,
            SMB_COM_TREE_CONNECT,
            SMB_COM_TREE_DISCONNECT,
            SMB_COM_NEGOTIATE,
            SMB_COM_SESSION_SETUP_ANDX,
            SMB_COM_LOGOFF_ANDX,
        ];
        for (i, &x) in c.iter().enumerate() {
            for &y in &c[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_smb2_commands_dense_0_to_18() {
        let c = [
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
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_nt_status_success_is_zero() {
        assert_eq!(NT_STATUS_SUCCESS, 0);
        assert_eq!(NT_STATUS_SUCCESS & NT_STATUS_SEVERITY_MASK, NT_STATUS_SEVERITY_SUCCESS);
    }

    #[test]
    fn test_nt_status_error_severity_set() {
        for &s in &[
            NT_STATUS_UNSUCCESSFUL,
            NT_STATUS_INVALID_HANDLE,
            NT_STATUS_ACCESS_DENIED,
            NT_STATUS_LOGON_FAILURE,
            NT_STATUS_OBJECT_NAME_NOT_FOUND,
            NT_STATUS_OBJECT_NAME_COLLISION,
        ] {
            assert_eq!(s & NT_STATUS_SEVERITY_MASK, NT_STATUS_SEVERITY_ERROR);
        }
    }

    #[test]
    fn test_severity_values_in_top_2_bits() {
        for s in [
            NT_STATUS_SEVERITY_SUCCESS,
            NT_STATUS_SEVERITY_INFO,
            NT_STATUS_SEVERITY_WARNING,
            NT_STATUS_SEVERITY_ERROR,
        ] {
            assert_eq!(s & !NT_STATUS_SEVERITY_MASK, 0);
        }
        // Mask covers exactly bits 30 and 31.
        assert_eq!(NT_STATUS_SEVERITY_MASK, 0b11 << 30);
    }

    #[test]
    fn test_smb_magic_signatures() {
        // Both start with the SMB ASCII tag in bytes 1..4.
        assert_eq!(&SMB_HEADER_MAGIC[1..], b"SMB");
        assert_eq!(&SMB2_HEADER_MAGIC[1..], b"SMB");
        // SMB1 begins with 0xFF, SMB2 with 0xFE.
        assert_eq!(SMB_HEADER_MAGIC[0], 0xFF);
        assert_eq!(SMB2_HEADER_MAGIC[0], 0xFE);
    }
}
