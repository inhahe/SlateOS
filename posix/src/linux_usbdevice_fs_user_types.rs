//! `<linux/usbdevice_fs.h>` — usbdevfs (`/dev/bus/usb/BBB/DDD`) ioctls.
//!
//! libusb, fwupd, and Wireshark's USBmon backend reach raw USB
//! transfers via the usbdevfs character device. The ioctls below
//! submit URBs, reap completions, claim/release interfaces, reset
//! ports, and query connection info.

// ---------------------------------------------------------------------------
// ioctl group letter
// ---------------------------------------------------------------------------

/// Magic letter for usbdevfs ioctls ('U').
pub const USBDEVFS_IOC_MAGIC: u8 = b'U';

// ---------------------------------------------------------------------------
// URB types
// ---------------------------------------------------------------------------

/// Isochronous (real-time audio/video).
pub const USBDEVFS_URB_TYPE_ISO: u8 = 0;
/// Interrupt (HID, keepalives).
pub const USBDEVFS_URB_TYPE_INTERRUPT: u8 = 1;
/// Control (CONTROL endpoint 0).
pub const USBDEVFS_URB_TYPE_CONTROL: u8 = 2;
/// Bulk (mass storage, ethernet).
pub const USBDEVFS_URB_TYPE_BULK: u8 = 3;

// ---------------------------------------------------------------------------
// URB flags
// ---------------------------------------------------------------------------

/// Short reads are not an error.
pub const USBDEVFS_URB_SHORT_NOT_OK: u32 = 0x01;
/// Use an isochronous ASAP scheduling policy.
pub const USBDEVFS_URB_ISO_ASAP: u32 = 0x02;
/// Disable BULK_CONTINUATION.
pub const USBDEVFS_URB_BULK_CONTINUATION: u32 = 0x04;
/// No FSBR (full-speed bandwidth reclaim).
pub const USBDEVFS_URB_NO_FSBR: u32 = 0x20;
/// Zero packet at the end of a bulk transfer.
pub const USBDEVFS_URB_ZERO_PACKET: u32 = 0x40;
/// No-interrupt on completion (kernel still completes, no IRQ).
pub const USBDEVFS_URB_NO_INTERRUPT: u32 = 0x80;

// ---------------------------------------------------------------------------
// ioctl numbers
// ---------------------------------------------------------------------------

/// `USBDEVFS_CONTROL` — issue a synchronous control transfer.
pub const USBDEVFS_CONTROL: u32 = 0xC005_5500;
/// `USBDEVFS_BULK` — synchronous bulk transfer.
pub const USBDEVFS_BULK: u32 = 0xC005_5502;
/// `USBDEVFS_RESETEP` — reset an endpoint.
pub const USBDEVFS_RESETEP: u32 = 0x8004_5503;
/// `USBDEVFS_SETINTERFACE` — set the active alt setting.
pub const USBDEVFS_SETINTERFACE: u32 = 0x8008_5504;
/// `USBDEVFS_SETCONFIGURATION` — set the device configuration.
pub const USBDEVFS_SETCONFIGURATION: u32 = 0x8004_5505;
/// `USBDEVFS_GETDRIVER` — query the driver bound to an interface.
pub const USBDEVFS_GETDRIVER: u32 = 0x4108_5508;
/// `USBDEVFS_SUBMITURB` — submit an asynchronous URB.
pub const USBDEVFS_SUBMITURB: u32 = 0x802A_550A;
/// `USBDEVFS_DISCARDURB` — cancel an in-flight URB.
pub const USBDEVFS_DISCARDURB: u32 = 0x0000_550B;
/// `USBDEVFS_REAPURB` — block until a URB completes.
pub const USBDEVFS_REAPURB: u32 = 0x4008_550C;
/// `USBDEVFS_REAPURBNDELAY` — non-blocking REAPURB.
pub const USBDEVFS_REAPURBNDELAY: u32 = 0x4008_550D;
/// `USBDEVFS_DISCSIGNAL` — register a disconnect signal.
pub const USBDEVFS_DISCSIGNAL: u32 = 0x8010_550E;
/// `USBDEVFS_CLAIMINTERFACE` — claim an interface.
pub const USBDEVFS_CLAIMINTERFACE: u32 = 0x8004_550F;
/// `USBDEVFS_RELEASEINTERFACE` — release an interface.
pub const USBDEVFS_RELEASEINTERFACE: u32 = 0x8004_5510;
/// `USBDEVFS_CONNECTINFO` — query bus speed and dev num.
pub const USBDEVFS_CONNECTINFO: u32 = 0x4008_5511;
/// `USBDEVFS_RESET` — port-level reset.
pub const USBDEVFS_RESET: u32 = 0x0000_5514;
/// `USBDEVFS_CLEAR_HALT` — clear a stalled endpoint.
pub const USBDEVFS_CLEAR_HALT: u32 = 0x8004_5515;
/// `USBDEVFS_DISCONNECT` — disconnect kernel driver.
pub const USBDEVFS_DISCONNECT: u32 = 0x0000_5516;
/// `USBDEVFS_CONNECT` — reconnect kernel driver.
pub const USBDEVFS_CONNECT: u32 = 0x0000_5517;

// ---------------------------------------------------------------------------
// Capabilities (USBDEVFS_GET_CAPABILITIES result bits)
// ---------------------------------------------------------------------------

/// Zero-copy via `mmap()` is supported.
pub const USBDEVFS_CAP_ZERO_PACKET: u32 = 0x01;
/// BULK_CONTINUATION supported.
pub const USBDEVFS_CAP_BULK_CONTINUATION: u32 = 0x02;
/// NO_PACKET_SIZE_LIM supported.
pub const USBDEVFS_CAP_NO_PACKET_SIZE_LIM: u32 = 0x04;
/// BULK_SCATTER_GATHER.
pub const USBDEVFS_CAP_BULK_SCATTER_GATHER: u32 = 0x08;
/// REAP_AFTER_DISCONNECT — REAPURB still works after EHOST.
pub const USBDEVFS_CAP_REAP_AFTER_DISCONNECT: u32 = 0x10;
/// MMAP supported.
pub const USBDEVFS_CAP_MMAP: u32 = 0x20;
/// DROP_PRIVILEGES — fd may drop CAP_SYS_RAWIO via DROP_PRIVS.
pub const USBDEVFS_CAP_DROP_PRIVILEGES: u32 = 0x40;
/// CONNINFO_EX (busnum + dev path).
pub const USBDEVFS_CAP_CONNINFO_EX: u32 = 0x80;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_letter_u() {
        assert_eq!(USBDEVFS_IOC_MAGIC, b'U');
    }

    #[test]
    fn test_urb_types_dense() {
        assert_eq!(USBDEVFS_URB_TYPE_ISO, 0);
        assert_eq!(USBDEVFS_URB_TYPE_INTERRUPT, 1);
        assert_eq!(USBDEVFS_URB_TYPE_CONTROL, 2);
        assert_eq!(USBDEVFS_URB_TYPE_BULK, 3);
    }

    #[test]
    fn test_urb_flags_pow2_distinct() {
        let f = [
            USBDEVFS_URB_SHORT_NOT_OK,
            USBDEVFS_URB_ISO_ASAP,
            USBDEVFS_URB_BULK_CONTINUATION,
            USBDEVFS_URB_NO_FSBR,
            USBDEVFS_URB_ZERO_PACKET,
            USBDEVFS_URB_NO_INTERRUPT,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct_and_use_letter_u() {
        let ops = [
            USBDEVFS_CONTROL,
            USBDEVFS_BULK,
            USBDEVFS_RESETEP,
            USBDEVFS_SETINTERFACE,
            USBDEVFS_SETCONFIGURATION,
            USBDEVFS_GETDRIVER,
            USBDEVFS_SUBMITURB,
            USBDEVFS_DISCARDURB,
            USBDEVFS_REAPURB,
            USBDEVFS_REAPURBNDELAY,
            USBDEVFS_DISCSIGNAL,
            USBDEVFS_CLAIMINTERFACE,
            USBDEVFS_RELEASEINTERFACE,
            USBDEVFS_CONNECTINFO,
            USBDEVFS_RESET,
            USBDEVFS_CLEAR_HALT,
            USBDEVFS_DISCONNECT,
            USBDEVFS_CONNECT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte 'U' (0x55) in bits 8..15.
            assert_eq!((ops[i] >> 8) & 0xff, b'U' as u32);
        }
    }

    #[test]
    fn test_caps_pow2_distinct() {
        let c = [
            USBDEVFS_CAP_ZERO_PACKET,
            USBDEVFS_CAP_BULK_CONTINUATION,
            USBDEVFS_CAP_NO_PACKET_SIZE_LIM,
            USBDEVFS_CAP_BULK_SCATTER_GATHER,
            USBDEVFS_CAP_REAP_AFTER_DISCONNECT,
            USBDEVFS_CAP_MMAP,
            USBDEVFS_CAP_DROP_PRIVILEGES,
            USBDEVFS_CAP_CONNINFO_EX,
        ];
        for &b in &c {
            assert!(b.is_power_of_two());
        }
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
    }
}
