//! `<linux/rpmsg.h>` — Remote processor messaging constants.
//!
//! RPMsg provides a message-passing mechanism between the main
//! application processor and remote processors (managed by
//! remoteproc). Each channel is identified by name and src/dst
//! endpoint addresses.

// ---------------------------------------------------------------------------
// RPMsg constants
// ---------------------------------------------------------------------------

/// Maximum RPMsg name length (including NUL).
pub const RPMSG_NAME_SIZE: usize = 32;

/// Any endpoint address (wildcard).
pub const RPMSG_ADDR_ANY: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// RPMsg name service announcement types
// ---------------------------------------------------------------------------

/// Create a new channel.
pub const RPMSG_NS_CREATE: u32 = 0;
/// Destroy a channel.
pub const RPMSG_NS_DESTROY: u32 = 1;

// ---------------------------------------------------------------------------
// RPMsg ioctl commands
// ---------------------------------------------------------------------------

/// Create endpoint ioctl.
pub const RPMSG_CREATE_EPT_IOCTL: u32 = 0xB501;
/// Destroy endpoint ioctl.
pub const RPMSG_DESTROY_EPT_IOCTL: u32 = 0xB502;
/// Create device ioctl (chardev).
pub const RPMSG_CREATE_DEV_IOCTL: u32 = 0xB503;
/// Release device ioctl.
pub const RPMSG_RELEASE_DEV_IOCTL: u32 = 0xB504;

// ---------------------------------------------------------------------------
// RPMsg header (virtio_rpmsg_hdr equivalent)
// ---------------------------------------------------------------------------

/// RPMsg message header.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RpmsgHdr {
    /// Source endpoint address.
    pub src: u32,
    /// Destination endpoint address.
    pub dst: u32,
    /// Reserved for future use.
    pub reserved: u32,
    /// Length of payload.
    pub len: u16,
    /// Message flags.
    pub flags: u16,
}

impl RpmsgHdr {
    /// Create a zeroed RPMsg header.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// RPMsg name service announcement.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RpmsgNsMsg {
    /// Channel name.
    pub name: [u8; RPMSG_NAME_SIZE],
    /// Endpoint address.
    pub addr: u32,
    /// Flags (RPMSG_NS_CREATE or RPMSG_NS_DESTROY).
    pub flags: u32,
}

impl RpmsgNsMsg {
    /// Create a zeroed name service message.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_size() {
        assert_eq!(RPMSG_NAME_SIZE, 32);
    }

    #[test]
    fn test_addr_any() {
        assert_eq!(RPMSG_ADDR_ANY, 0xFFFF_FFFF);
    }

    #[test]
    fn test_ns_types() {
        assert_eq!(RPMSG_NS_CREATE, 0);
        assert_eq!(RPMSG_NS_DESTROY, 1);
    }

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            RPMSG_CREATE_EPT_IOCTL, RPMSG_DESTROY_EPT_IOCTL,
            RPMSG_CREATE_DEV_IOCTL, RPMSG_RELEASE_DEV_IOCTL,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_rpmsg_hdr_size() {
        assert_eq!(core::mem::size_of::<RpmsgHdr>(), 16);
    }

    #[test]
    fn test_rpmsg_ns_msg_size() {
        assert_eq!(core::mem::size_of::<RpmsgNsMsg>(), 40);
    }

    #[test]
    fn test_rpmsg_hdr_zeroed() {
        let hdr = RpmsgHdr::zeroed();
        assert_eq!(hdr.src, 0);
        assert_eq!(hdr.dst, 0);
        assert_eq!(hdr.len, 0);
        assert_eq!(hdr.flags, 0);
    }
}
