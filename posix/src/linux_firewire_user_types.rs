//! `<linux/firewire-cdev.h>` — IEEE-1394 (FireWire) userspace char-dev ABI.
//!
//! Modern Linux exposes FireWire via `/dev/fw0`, `/dev/fw1` … and
//! libraw1394's userspace clients (juju stack) speak to the kernel
//! using the cdev event protocol. The ioctls and event codes here are
//! the userspace surface for DV camcorder capture, AV/C control, and
//! SBP-2 disk targets.

// ---------------------------------------------------------------------------
// Device path
// ---------------------------------------------------------------------------

/// Character-device prefix: `/dev/fw0`, `/dev/fw1`, …
pub const FW_DEV_PREFIX: &str = "/dev/fw";

// ---------------------------------------------------------------------------
// Protocol version
// ---------------------------------------------------------------------------

/// Earliest cdev API version (juju merge in 2.6.22).
pub const FW_CDEV_VERSION_MIN: u32 = 1;
/// Current cdev API version.
pub const FW_CDEV_VERSION: u32 = 5;

// ---------------------------------------------------------------------------
// ioctl numbers (group letter '#' = 0x23)
// ---------------------------------------------------------------------------

/// `FW_CDEV_IOC_GET_INFO`.
pub const FW_CDEV_IOC_GET_INFO: u32 = 0xC040_2300;
/// `FW_CDEV_IOC_SEND_REQUEST`.
pub const FW_CDEV_IOC_SEND_REQUEST: u32 = 0x4030_2301;
/// `FW_CDEV_IOC_ALLOCATE`.
pub const FW_CDEV_IOC_ALLOCATE: u32 = 0xC020_2302;
/// `FW_CDEV_IOC_DEALLOCATE`.
pub const FW_CDEV_IOC_DEALLOCATE: u32 = 0x4004_2303;
/// `FW_CDEV_IOC_SEND_RESPONSE`.
pub const FW_CDEV_IOC_SEND_RESPONSE: u32 = 0x4018_2304;
/// `FW_CDEV_IOC_INITIATE_BUS_RESET`.
pub const FW_CDEV_IOC_INITIATE_BUS_RESET: u32 = 0x4004_2305;

// ---------------------------------------------------------------------------
// Event types (struct fw_cdev_event_common.type)
// ---------------------------------------------------------------------------

/// Bus reset.
pub const FW_CDEV_EVENT_BUS_RESET: u32 = 0x00;
/// Response received.
pub const FW_CDEV_EVENT_RESPONSE: u32 = 0x01;
/// Incoming request.
pub const FW_CDEV_EVENT_REQUEST: u32 = 0x02;
/// Isochronous interrupt.
pub const FW_CDEV_EVENT_ISO_INTERRUPT: u32 = 0x03;
/// Iso resource allocated.
pub const FW_CDEV_EVENT_ISO_RESOURCE_ALLOCATED: u32 = 0x04;
/// Iso resource deallocated.
pub const FW_CDEV_EVENT_ISO_RESOURCE_DEALLOCATED: u32 = 0x05;
/// Request2 with destination address.
pub const FW_CDEV_EVENT_REQUEST2: u32 = 0x06;
/// PHY packet sent.
pub const FW_CDEV_EVENT_PHY_PACKET_SENT: u32 = 0x07;
/// PHY packet received.
pub const FW_CDEV_EVENT_PHY_PACKET_RECEIVED: u32 = 0x08;
/// Iso interrupt for multichannel context.
pub const FW_CDEV_EVENT_ISO_INTERRUPT_MULTICHANNEL: u32 = 0x09;

// ---------------------------------------------------------------------------
// Transaction codes (tcode field)
// ---------------------------------------------------------------------------

/// Write quadlet request.
pub const TCODE_WRITE_QUADLET_REQUEST: u32 = 0x0;
/// Write block request.
pub const TCODE_WRITE_BLOCK_REQUEST: u32 = 0x1;
/// Write response.
pub const TCODE_WRITE_RESPONSE: u32 = 0x2;
/// Read quadlet request.
pub const TCODE_READ_QUADLET_REQUEST: u32 = 0x4;
/// Read block request.
pub const TCODE_READ_BLOCK_REQUEST: u32 = 0x5;
/// Read quadlet response.
pub const TCODE_READ_QUADLET_RESPONSE: u32 = 0x6;
/// Read block response.
pub const TCODE_READ_BLOCK_RESPONSE: u32 = 0x7;
/// Cycle start (isoch sync).
pub const TCODE_CYCLE_START: u32 = 0x8;
/// Lock request (e.g. compare-and-swap).
pub const TCODE_LOCK_REQUEST: u32 = 0x9;
/// Streaming.
pub const TCODE_STREAM_DATA: u32 = 0xa;
/// Lock response.
pub const TCODE_LOCK_RESPONSE: u32 = 0xb;

// ---------------------------------------------------------------------------
// Response codes (rcode field)
// ---------------------------------------------------------------------------

/// Completed successfully.
pub const RCODE_COMPLETE: u32 = 0x0;
/// Conflict error.
pub const RCODE_CONFLICT_ERROR: u32 = 0x4;
/// Data error.
pub const RCODE_DATA_ERROR: u32 = 0x5;
/// Type error.
pub const RCODE_TYPE_ERROR: u32 = 0x6;
/// Address error.
pub const RCODE_ADDRESS_ERROR: u32 = 0x7;
/// Send error (kernel-only).
pub const RCODE_SEND_ERROR: u32 = 0x10;
/// Cancelled.
pub const RCODE_CANCELLED: u32 = 0x11;
/// Bus reset cancelled.
pub const RCODE_BUSY: u32 = 0x12;
/// Generation mismatch.
pub const RCODE_GENERATION: u32 = 0x13;
/// No ack.
pub const RCODE_NO_ACK: u32 = 0x14;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_prefix() {
        assert_eq!(FW_DEV_PREFIX, "/dev/fw");
    }

    #[test]
    fn test_version_monotonic() {
        assert!(FW_CDEV_VERSION_MIN <= FW_CDEV_VERSION);
        assert_eq!(FW_CDEV_VERSION_MIN, 1);
    }

    #[test]
    fn test_ioctls_distinct_and_use_letter_hash() {
        let ops = [
            FW_CDEV_IOC_GET_INFO,
            FW_CDEV_IOC_SEND_REQUEST,
            FW_CDEV_IOC_ALLOCATE,
            FW_CDEV_IOC_DEALLOCATE,
            FW_CDEV_IOC_SEND_RESPONSE,
            FW_CDEV_IOC_INITIATE_BUS_RESET,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte '#' (0x23) in bits 8..15.
            assert_eq!((ops[i] >> 8) & 0xff, b'#' as u32);
        }
    }

    #[test]
    fn test_events_distinct() {
        let e = [
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
        for (i, &v) in e.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_tcodes_distinct() {
        let t = [
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
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
            // Transaction codes are 4-bit values.
            assert!(t[i] < 16);
        }
    }

    #[test]
    fn test_rcodes_distinct_and_in_range() {
        let r = [
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
        for i in 0..r.len() {
            for j in (i + 1)..r.len() {
                assert_ne!(r[i], r[j]);
            }
        }
        // On-wire rcodes occupy 4 bits (0..7 used); kernel-only ones
        // use the 0x10+ range to avoid collision.
        assert!(RCODE_ADDRESS_ERROR < 0x10);
        assert!(RCODE_SEND_ERROR >= 0x10);
    }
}
