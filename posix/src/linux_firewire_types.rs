//! `<linux/firewire-cdev.h>` — IEEE 1394 (FireWire) constants.
//!
//! FireWire constants covering bus speeds, transaction codes,
//! event types, and response codes.

// ---------------------------------------------------------------------------
// Bus speeds (SCODE_*)
// ---------------------------------------------------------------------------

/// S100 (100 Mbps).
pub const SCODE_100: u32 = 0;
/// S200 (200 Mbps).
pub const SCODE_200: u32 = 1;
/// S400 (400 Mbps).
pub const SCODE_400: u32 = 2;
/// S800 (800 Mbps).
pub const SCODE_800: u32 = 3;
/// S1600 (1600 Mbps).
pub const SCODE_1600: u32 = 4;
/// S3200 (3200 Mbps).
pub const SCODE_3200: u32 = 5;

// ---------------------------------------------------------------------------
// Transaction codes (TCODE_*)
// ---------------------------------------------------------------------------

/// Write request (quadlet).
pub const TCODE_WRITE_QUADLET_REQUEST: u32 = 0;
/// Write request (block).
pub const TCODE_WRITE_BLOCK_REQUEST: u32 = 1;
/// Write response.
pub const TCODE_WRITE_RESPONSE: u32 = 2;
/// Read request (quadlet).
pub const TCODE_READ_QUADLET_REQUEST: u32 = 4;
/// Read request (block).
pub const TCODE_READ_BLOCK_REQUEST: u32 = 5;
/// Read response (quadlet).
pub const TCODE_READ_QUADLET_RESPONSE: u32 = 6;
/// Read response (block).
pub const TCODE_READ_BLOCK_RESPONSE: u32 = 7;
/// Lock request.
pub const TCODE_LOCK_REQUEST: u32 = 9;
/// Stream data.
pub const TCODE_STREAM_DATA: u32 = 10;
/// Lock response.
pub const TCODE_LOCK_RESPONSE: u32 = 11;

// ---------------------------------------------------------------------------
// Response codes (RCODE_*)
// ---------------------------------------------------------------------------

/// Complete.
pub const RCODE_COMPLETE: u32 = 0x0;
/// Conflict error.
pub const RCODE_CONFLICT_ERROR: u32 = 0x4;
/// Data error.
pub const RCODE_DATA_ERROR: u32 = 0x5;
/// Type error.
pub const RCODE_TYPE_ERROR: u32 = 0x6;
/// Address error.
pub const RCODE_ADDRESS_ERROR: u32 = 0x7;

// ---------------------------------------------------------------------------
// Event types (FW_CDEV_EVENT_*)
// ---------------------------------------------------------------------------

/// Bus reset.
pub const FW_CDEV_EVENT_BUS_RESET: u32 = 0x00;
/// Response.
pub const FW_CDEV_EVENT_RESPONSE: u32 = 0x01;
/// Request (legacy).
pub const FW_CDEV_EVENT_REQUEST: u32 = 0x02;
/// Iso interrupt.
pub const FW_CDEV_EVENT_ISO_INTERRUPT: u32 = 0x03;
/// Iso interrupt (multicast).
pub const FW_CDEV_EVENT_ISO_INTERRUPT_MULTICHANNEL: u32 = 0x04;
/// PHY packet.
pub const FW_CDEV_EVENT_PHY_PACKET_SENT: u32 = 0x05;
/// PHY packet received.
pub const FW_CDEV_EVENT_PHY_PACKET_RECEIVED: u32 = 0x06;
/// Request2.
pub const FW_CDEV_EVENT_REQUEST2: u32 = 0x07;
/// Iso resource allocated.
pub const FW_CDEV_EVENT_ISO_RESOURCE_ALLOCATED: u32 = 0x08;
/// Iso resource deallocated.
pub const FW_CDEV_EVENT_ISO_RESOURCE_DEALLOCATED: u32 = 0x09;

// ---------------------------------------------------------------------------
// Lock types
// ---------------------------------------------------------------------------

/// Mask swap.
pub const FW_CDEV_ISO_CONTEXT_TRANSMIT: u32 = 0;
/// Receive.
pub const FW_CDEV_ISO_CONTEXT_RECEIVE: u32 = 1;
/// Receive multichannel.
pub const FW_CDEV_ISO_CONTEXT_RECEIVE_MULTICHANNEL: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speeds_sequential() {
        assert_eq!(SCODE_100, 0);
        assert_eq!(SCODE_200, 1);
        assert_eq!(SCODE_3200, 5);
    }

    #[test]
    fn test_tcodes_distinct() {
        let tcodes = [
            TCODE_WRITE_QUADLET_REQUEST, TCODE_WRITE_BLOCK_REQUEST,
            TCODE_WRITE_RESPONSE, TCODE_READ_QUADLET_REQUEST,
            TCODE_READ_BLOCK_REQUEST, TCODE_READ_QUADLET_RESPONSE,
            TCODE_READ_BLOCK_RESPONSE, TCODE_LOCK_REQUEST,
            TCODE_STREAM_DATA, TCODE_LOCK_RESPONSE,
        ];
        for i in 0..tcodes.len() {
            for j in (i + 1)..tcodes.len() {
                assert_ne!(tcodes[i], tcodes[j]);
            }
        }
    }

    #[test]
    fn test_rcodes_distinct() {
        let rcodes = [
            RCODE_COMPLETE, RCODE_CONFLICT_ERROR,
            RCODE_DATA_ERROR, RCODE_TYPE_ERROR,
            RCODE_ADDRESS_ERROR,
        ];
        for i in 0..rcodes.len() {
            for j in (i + 1)..rcodes.len() {
                assert_ne!(rcodes[i], rcodes[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            FW_CDEV_EVENT_BUS_RESET, FW_CDEV_EVENT_RESPONSE,
            FW_CDEV_EVENT_REQUEST, FW_CDEV_EVENT_ISO_INTERRUPT,
            FW_CDEV_EVENT_ISO_INTERRUPT_MULTICHANNEL,
            FW_CDEV_EVENT_PHY_PACKET_SENT,
            FW_CDEV_EVENT_PHY_PACKET_RECEIVED,
            FW_CDEV_EVENT_REQUEST2,
            FW_CDEV_EVENT_ISO_RESOURCE_ALLOCATED,
            FW_CDEV_EVENT_ISO_RESOURCE_DEALLOCATED,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_iso_contexts() {
        assert_eq!(FW_CDEV_ISO_CONTEXT_TRANSMIT, 0);
        assert_eq!(FW_CDEV_ISO_CONTEXT_RECEIVE, 1);
        assert_eq!(FW_CDEV_ISO_CONTEXT_RECEIVE_MULTICHANNEL, 2);
    }

    #[test]
    fn test_rcode_complete() {
        assert_eq!(RCODE_COMPLETE, 0);
    }
}
