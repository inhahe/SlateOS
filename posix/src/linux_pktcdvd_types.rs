//! `<linux/pktcdvd.h>` — packet writing for CD-RW/DVD-RW drives.
//!
//! The pktcdvd driver wraps a SCSI/ATAPI optical writer in a
//! buffered "packet" layer so userspace can treat a CD-RW as a
//! random-access block device. pktsetup, growisofs, and cdrkit
//! talk to `/dev/pktcdvd/control` and `/dev/pktcdvd/N` via the
//! constants below.

// ---------------------------------------------------------------------------
// Driver limits
// ---------------------------------------------------------------------------

/// Maximum number of packet devices (struct file_operations table).
pub const MAX_WRITERS: u32 = 8;
/// Default write-buffer size in physical blocks.
pub const PACKET_WAKEUP_NS: u32 = 32;

// ---------------------------------------------------------------------------
// /dev/pktcdvd/control ioctls
// ---------------------------------------------------------------------------

/// Magic letter for pktcdvd control ioctls.
pub const PACKET_IOCTL_MAGIC: u8 = b'X';

/// `PACKET_SETUP_DEV` — attach a block dev to a packet number.
pub const PACKET_SETUP_DEV: u32 = 0x4000_5801;
/// `PACKET_TEARDOWN_DEV` — detach a packet number.
pub const PACKET_TEARDOWN_DEV: u32 = 0x4000_5802;
/// `PACKET_CTRL_CMD` — generic ctrl request (struct pkt_ctrl_command).
pub const PACKET_CTRL_CMD: u32 = 0xc010_5801;

// ---------------------------------------------------------------------------
// pkt_ctrl_command.command codes
// ---------------------------------------------------------------------------

/// Report driver status for a packet device.
pub const PKT_CTRL_CMD_STATUS: u32 = 0;
/// Reset (re-initialise) a packet device.
pub const PKT_CTRL_CMD_RESET: u32 = 1;
/// Set up a packet device — same as PACKET_SETUP_DEV via the new path.
pub const PKT_CTRL_CMD_SETUP: u32 = 2;
/// Tear down a packet device.
pub const PKT_CTRL_CMD_TEARDOWN: u32 = 3;

// ---------------------------------------------------------------------------
// MRW (Mount-Rainier-reWritable) feature bits / disc states
// ---------------------------------------------------------------------------

/// MRW disc present.
pub const PACKET_MRW_PRESENT: u32 = 1 << 0;
/// MRW formatting in progress.
pub const PACKET_MRW_FORMAT_INPROC: u32 = 1 << 1;
/// MRW background format complete.
pub const PACKET_MRW_FORMAT_DONE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// pkt_ctrl_command status bits
// ---------------------------------------------------------------------------

/// Disc is blank (formattable).
pub const PACKET_DISC_BLANK: u32 = 0x0001;
/// Disc is appendable.
pub const PACKET_DISC_APPENDABLE: u32 = 0x0002;
/// Disc is finalised.
pub const PACKET_DISC_FINALIZED: u32 = 0x0004;
/// Disc is read-only.
pub const PACKET_DISC_READONLY: u32 = 0x0008;
/// Disc unknown / no media.
pub const PACKET_DISC_NONE: u32 = 0x0010;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_writers_sane() {
        // 8 writers is the historical limit since 2.6.
        assert_eq!(MAX_WRITERS, 8);
        assert!(MAX_WRITERS.is_power_of_two());
    }

    #[test]
    fn test_magic_letter_x() {
        assert_eq!(PACKET_IOCTL_MAGIC, b'X');
    }

    #[test]
    fn test_ioctls_distinct_and_use_magic_x() {
        let ops = [PACKET_SETUP_DEV, PACKET_TEARDOWN_DEV, PACKET_CTRL_CMD];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte 'X' (0x58) in bits 8..15.
            assert_eq!((ops[i] >> 8) & 0xff, b'X' as u32);
        }
    }

    #[test]
    fn test_ctrl_subcmds_dense() {
        let c = [
            PKT_CTRL_CMD_STATUS,
            PKT_CTRL_CMD_RESET,
            PKT_CTRL_CMD_SETUP,
            PKT_CTRL_CMD_TEARDOWN,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_mrw_bits_distinct_pow2() {
        let m = [
            PACKET_MRW_PRESENT,
            PACKET_MRW_FORMAT_INPROC,
            PACKET_MRW_FORMAT_DONE,
        ];
        for &b in &m {
            assert!(b.is_power_of_two());
        }
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
    }

    #[test]
    fn test_disc_status_bits_distinct_pow2() {
        let s = [
            PACKET_DISC_BLANK,
            PACKET_DISC_APPENDABLE,
            PACKET_DISC_FINALIZED,
            PACKET_DISC_READONLY,
            PACKET_DISC_NONE,
        ];
        for &b in &s {
            assert!(b.is_power_of_two());
        }
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
    }
}
