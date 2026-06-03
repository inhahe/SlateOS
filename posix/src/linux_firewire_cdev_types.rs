//! `<linux/firewire-cdev.h>` — FireWire character-device protocol.
//!
//! Constants for the IEEE 1394 (FireWire) character-device userspace
//! interface (`/dev/fw*`) — used by libraw1394 and AV/C applications
//! to send asynchronous and isochronous transactions.

// ---------------------------------------------------------------------------
// fw_cdev_event.type — event type IDs from kernel to userspace
// ---------------------------------------------------------------------------

/// Bus reset.
pub const FW_CDEV_EVENT_BUS_RESET: u32 = 0x00;
/// Async response received.
pub const FW_CDEV_EVENT_RESPONSE: u32 = 0x01;
/// Async request received.
pub const FW_CDEV_EVENT_REQUEST: u32 = 0x02;
/// Iso completion.
pub const FW_CDEV_EVENT_ISO_INTERRUPT: u32 = 0x03;
/// Async request received (deprecated layout).
pub const FW_CDEV_EVENT_ISO_RESOURCE_ALLOCATED: u32 = 0x04;
/// Iso resource deallocated.
pub const FW_CDEV_EVENT_ISO_RESOURCE_DEALLOCATED: u32 = 0x05;
/// Request2 — extended async request format.
pub const FW_CDEV_EVENT_REQUEST2: u32 = 0x06;
/// PHY packet sent.
pub const FW_CDEV_EVENT_PHY_PACKET_SENT: u32 = 0x07;
/// PHY packet received.
pub const FW_CDEV_EVENT_PHY_PACKET_RECEIVED: u32 = 0x08;
/// Iso interrupt with multichannel header.
pub const FW_CDEV_EVENT_ISO_INTERRUPT_MULTICHANNEL: u32 = 0x09;

// ---------------------------------------------------------------------------
// Transaction codes (tcode) used in async requests
// ---------------------------------------------------------------------------

/// Quadlet write request.
pub const TCODE_WRITE_QUADLET_REQUEST: u32 = 0x0;
/// Block write request.
pub const TCODE_WRITE_BLOCK_REQUEST: u32 = 0x1;
/// Write response.
pub const TCODE_WRITE_RESPONSE: u32 = 0x2;
/// Quadlet read request.
pub const TCODE_READ_QUADLET_REQUEST: u32 = 0x4;
/// Block read request.
pub const TCODE_READ_BLOCK_REQUEST: u32 = 0x5;
/// Quadlet read response.
pub const TCODE_READ_QUADLET_RESPONSE: u32 = 0x6;
/// Block read response.
pub const TCODE_READ_BLOCK_RESPONSE: u32 = 0x7;
/// Cycle start.
pub const TCODE_CYCLE_START: u32 = 0x8;
/// Lock request.
pub const TCODE_LOCK_REQUEST: u32 = 0x9;
/// Stream data.
pub const TCODE_STREAM_DATA: u32 = 0xA;
/// Lock response.
pub const TCODE_LOCK_RESPONSE: u32 = 0xB;

// ---------------------------------------------------------------------------
// Response status codes (rcode)
// ---------------------------------------------------------------------------

/// Transaction completed.
pub const RCODE_COMPLETE: u32 = 0x0;
/// Conflict error.
pub const RCODE_CONFLICT_ERROR: u32 = 0x4;
/// Data error.
pub const RCODE_DATA_ERROR: u32 = 0x5;
/// Type error.
pub const RCODE_TYPE_ERROR: u32 = 0x6;
/// Address error.
pub const RCODE_ADDRESS_ERROR: u32 = 0x7;
/// Send error (sender hung up).
pub const RCODE_SEND_ERROR: u32 = 0x10;
/// Cancelled (split-timeout etc.).
pub const RCODE_CANCELLED: u32 = 0x11;
/// Busy (retry-limit exceeded).
pub const RCODE_BUSY: u32 = 0x12;
/// Generation count changed mid-transaction.
pub const RCODE_GENERATION: u32 = 0x13;
/// No ACK received.
pub const RCODE_NO_ACK: u32 = 0x14;

// ---------------------------------------------------------------------------
// Iso resource flags (struct fw_cdev_allocate_iso_resource.flags)
// ---------------------------------------------------------------------------

/// Allocate iso channel.
pub const FW_CDEV_ISO_CONTEXT_TRANSMIT: u32 = 0;
/// Receive in packet-per-buffer mode.
pub const FW_CDEV_ISO_CONTEXT_RECEIVE: u32 = 1;
/// Receive multi-channel.
pub const FW_CDEV_ISO_CONTEXT_RECEIVE_MULTICHANNEL: u32 = 2;

// ---------------------------------------------------------------------------
// API version
// ---------------------------------------------------------------------------

/// Current API version reported by FW_CDEV_IOC_GET_INFO.
pub const FW_CDEV_VERSION: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct() {
        let events = [
            FW_CDEV_EVENT_BUS_RESET,
            FW_CDEV_EVENT_RESPONSE,
            FW_CDEV_EVENT_REQUEST,
            FW_CDEV_EVENT_ISO_INTERRUPT,
            FW_CDEV_EVENT_ISO_RESOURCE_ALLOCATED,
            FW_CDEV_EVENT_ISO_RESOURCE_DEALLOCATED,
            FW_CDEV_EVENT_REQUEST2,
            FW_CDEV_EVENT_PHY_PACKET_SENT,
            FW_CDEV_EVENT_PHY_PACKET_RECEIVED,
            FW_CDEV_EVENT_ISO_INTERRUPT_MULTICHANNEL,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_tcodes_distinct() {
        let tcodes = [
            TCODE_WRITE_QUADLET_REQUEST,
            TCODE_WRITE_BLOCK_REQUEST,
            TCODE_WRITE_RESPONSE,
            TCODE_READ_QUADLET_REQUEST,
            TCODE_READ_BLOCK_REQUEST,
            TCODE_READ_QUADLET_RESPONSE,
            TCODE_READ_BLOCK_RESPONSE,
            TCODE_CYCLE_START,
            TCODE_LOCK_REQUEST,
            TCODE_STREAM_DATA,
            TCODE_LOCK_RESPONSE,
        ];
        for i in 0..tcodes.len() {
            for j in (i + 1)..tcodes.len() {
                assert_ne!(tcodes[i], tcodes[j]);
            }
        }
        // tcodes are 4-bit fields in the actual 1394 header — none must
        // exceed 0xF.
        for &t in &tcodes {
            assert!(t < 0x10);
        }
    }

    #[test]
    fn test_rcodes_distinct() {
        let rcodes = [
            RCODE_COMPLETE,
            RCODE_CONFLICT_ERROR,
            RCODE_DATA_ERROR,
            RCODE_TYPE_ERROR,
            RCODE_ADDRESS_ERROR,
            RCODE_SEND_ERROR,
            RCODE_CANCELLED,
            RCODE_BUSY,
            RCODE_GENERATION,
            RCODE_NO_ACK,
        ];
        for i in 0..rcodes.len() {
            for j in (i + 1)..rcodes.len() {
                assert_ne!(rcodes[i], rcodes[j]);
            }
        }
    }

    #[test]
    fn test_iso_context_distinct() {
        let ctxs = [
            FW_CDEV_ISO_CONTEXT_TRANSMIT,
            FW_CDEV_ISO_CONTEXT_RECEIVE,
            FW_CDEV_ISO_CONTEXT_RECEIVE_MULTICHANNEL,
        ];
        for i in 0..ctxs.len() {
            for j in (i + 1)..ctxs.len() {
                assert_ne!(ctxs[i], ctxs[j]);
            }
        }
    }

    #[test]
    fn test_api_version_positive() {
        assert!(FW_CDEV_VERSION >= 1);
    }
}
