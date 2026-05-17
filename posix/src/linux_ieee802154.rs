//! `<linux/ieee802154.h>` — IEEE 802.15.4 (LR-WPAN) constants.
//!
//! IEEE 802.15.4 defines the physical and MAC layers for low-rate
//! wireless personal area networks (LR-WPANs). It is the basis
//! for Zigbee, Thread, 6LoWPAN, and other IoT mesh protocols.

// ---------------------------------------------------------------------------
// Protocol family
// ---------------------------------------------------------------------------

/// IEEE 802.15.4 protocol family.
pub const PF_IEEE802154: u16 = 36;
/// IEEE 802.15.4 address family.
pub const AF_IEEE802154: u16 = 36;

// ---------------------------------------------------------------------------
// Address modes
// ---------------------------------------------------------------------------

/// No address.
pub const IEEE802154_ADDR_NONE: u8 = 0;
/// Short (16-bit) address.
pub const IEEE802154_ADDR_SHORT: u8 = 2;
/// Extended (64-bit / EUI-64) address.
pub const IEEE802154_ADDR_LONG: u8 = 3;

// ---------------------------------------------------------------------------
// Frame types
// ---------------------------------------------------------------------------

/// Beacon frame.
pub const IEEE802154_FC_TYPE_BEACON: u8 = 0;
/// Data frame.
pub const IEEE802154_FC_TYPE_DATA: u8 = 1;
/// Acknowledgment frame.
pub const IEEE802154_FC_TYPE_ACK: u8 = 2;
/// MAC command frame.
pub const IEEE802154_FC_TYPE_MAC_CMD: u8 = 3;

// ---------------------------------------------------------------------------
// MAC commands
// ---------------------------------------------------------------------------

/// Association request.
pub const IEEE802154_CMD_ASSOC_REQ: u8 = 1;
/// Association response.
pub const IEEE802154_CMD_ASSOC_RESP: u8 = 2;
/// Disassociation notification.
pub const IEEE802154_CMD_DISASSOC: u8 = 3;
/// Data request.
pub const IEEE802154_CMD_DATA_REQ: u8 = 4;
/// PAN ID conflict.
pub const IEEE802154_CMD_PAN_CONFLICT: u8 = 5;
/// Orphan notification.
pub const IEEE802154_CMD_ORPHAN: u8 = 6;
/// Beacon request.
pub const IEEE802154_CMD_BEACON_REQ: u8 = 7;
/// Coordinator realignment.
pub const IEEE802154_CMD_COORD_REALIGN: u8 = 8;

// ---------------------------------------------------------------------------
// Special addresses
// ---------------------------------------------------------------------------

/// Broadcast short address.
pub const IEEE802154_BROADCAST_ADDR: u16 = 0xFFFF;
/// Unassigned short address.
pub const IEEE802154_UNASSIGNED_ADDR: u16 = 0xFFFE;
/// Broadcast PAN ID.
pub const IEEE802154_BROADCAST_PAN: u16 = 0xFFFF;

// ---------------------------------------------------------------------------
// PHY channels / pages
// ---------------------------------------------------------------------------

/// Channel page 0 (2.4 GHz O-QPSK).
pub const IEEE802154_CHANNEL_PAGE_0: u8 = 0;
/// Channel page 1 (868/915 MHz ASK).
pub const IEEE802154_CHANNEL_PAGE_1: u8 = 1;
/// Channel page 2 (868/915 MHz O-QPSK).
pub const IEEE802154_CHANNEL_PAGE_2: u8 = 2;
/// Minimum channel (page 0, 2.4 GHz).
pub const IEEE802154_MIN_CHANNEL: u8 = 11;
/// Maximum channel (page 0, 2.4 GHz).
pub const IEEE802154_MAX_CHANNEL: u8 = 26;

// ---------------------------------------------------------------------------
// Frame control flags
// ---------------------------------------------------------------------------

/// Frame pending bit.
pub const IEEE802154_FC_FRAME_PENDING: u16 = 1 << 4;
/// ACK request bit.
pub const IEEE802154_FC_ACK_REQ: u16 = 1 << 5;
/// PAN ID compression.
pub const IEEE802154_FC_INTRA_PAN: u16 = 1 << 6;
/// Security enabled.
pub const IEEE802154_FC_SECURITY: u16 = 1 << 3;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum PHY frame length (bytes).
pub const IEEE802154_MTU: u16 = 127;
/// Minimum frame length (header + FCS).
pub const IEEE802154_MIN_FRAME: u8 = 5;
/// FCS (Frame Check Sequence) length.
pub const IEEE802154_FCS_LEN: u8 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_family() {
        assert_eq!(PF_IEEE802154, AF_IEEE802154);
    }

    #[test]
    fn test_addr_modes_distinct() {
        let modes = [
            IEEE802154_ADDR_NONE, IEEE802154_ADDR_SHORT,
            IEEE802154_ADDR_LONG,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_frame_types_distinct() {
        let types = [
            IEEE802154_FC_TYPE_BEACON, IEEE802154_FC_TYPE_DATA,
            IEEE802154_FC_TYPE_ACK, IEEE802154_FC_TYPE_MAC_CMD,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_mac_commands_distinct() {
        let cmds = [
            IEEE802154_CMD_ASSOC_REQ, IEEE802154_CMD_ASSOC_RESP,
            IEEE802154_CMD_DISASSOC, IEEE802154_CMD_DATA_REQ,
            IEEE802154_CMD_PAN_CONFLICT, IEEE802154_CMD_ORPHAN,
            IEEE802154_CMD_BEACON_REQ, IEEE802154_CMD_COORD_REALIGN,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_channel_range() {
        assert!(IEEE802154_MIN_CHANNEL < IEEE802154_MAX_CHANNEL);
        assert_eq!(IEEE802154_MAX_CHANNEL - IEEE802154_MIN_CHANNEL + 1, 16);
    }

    #[test]
    fn test_mtu() {
        assert_eq!(IEEE802154_MTU, 127);
    }
}
