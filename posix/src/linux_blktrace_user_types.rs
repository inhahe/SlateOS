//! `<linux/blktrace.h>` — blktrace per-event record layout.
//!
//! Each event written to the relay buffer is a fixed-size
//! `struct blk_io_trace` followed by an optional variable-length
//! payload. This module fixes the header byte layout, the per-CPU
//! buffer parameters, and the field-shift constants `blkparse` uses
//! to demultiplex the packed `action` word.

// ---------------------------------------------------------------------------
// Per-event header geometry
// ---------------------------------------------------------------------------

/// Fixed `struct blk_io_trace` size in bytes.
///
/// Field layout (all little-endian on host):
///   u32 magic; u32 sequence; u64 time; u64 sector;
///   u32 bytes; u32 action; u32 pid; u32 device; u32 cpu;
///   u16 error; u16 pdu_len; u32 padding
pub const BLK_IO_TRACE_HEADER_SIZE: usize = 48;

/// Maximum `pdu_len` payload bytes following the fixed header.
pub const BLK_IO_TRACE_PDU_MAX: usize = 64;

/// Total worst-case event size.
pub const BLK_IO_TRACE_RECORD_MAX: usize =
    BLK_IO_TRACE_HEADER_SIZE + BLK_IO_TRACE_PDU_MAX;

// ---------------------------------------------------------------------------
// Packed-`action` word geometry
// ---------------------------------------------------------------------------

/// Low byte of `action` carries the BLK_TA_* code.
pub const BLK_TA_MASK: u32 = 0xFF;

/// Bit position where the BLK_TC_* class bits begin.
pub const BLK_TC_SHIFT: u32 = 16;

/// 16-bit mask covering all BLK_TC_* classes.
pub const BLK_TC_MASK: u32 = 0xFFFF;

// ---------------------------------------------------------------------------
// Per-CPU buffer parameters (passed to `BLKTRACESETUP`)
// ---------------------------------------------------------------------------

/// Default sub-buffer size — 512 KiB.
pub const BLK_DEFAULT_BUF_SIZE: u32 = 512 * 1024;

/// Default sub-buffer count per CPU.
pub const BLK_DEFAULT_BUF_NR: u32 = 4;

/// Minimum sub-buffer size — one 16 KiB page.
pub const BLK_MIN_BUF_SIZE: u32 = 16 * 1024;

/// Minimum sub-buffer count.
pub const BLK_MIN_BUF_NR: u32 = 2;

/// Maximum CPUs supported by a single trace session.
pub const BLK_MAX_CPUS: u32 = 4_096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_size_matches_field_layout() {
        // magic(4) + seq(4) + time(8) + sector(8) + bytes(4) + action(4)
        //   + pid(4) + dev(4) + cpu(4) + error(2) + pdu_len(2) + pad(4) = 48
        assert_eq!(BLK_IO_TRACE_HEADER_SIZE, 48);
        // Header is 8-byte aligned.
        assert_eq!(BLK_IO_TRACE_HEADER_SIZE % 8, 0);
    }

    #[test]
    fn test_pdu_and_record_sizes() {
        assert_eq!(BLK_IO_TRACE_PDU_MAX, 64);
        assert_eq!(BLK_IO_TRACE_RECORD_MAX, 48 + 64);
        // Worst-case record is 112 bytes.
        assert_eq!(BLK_IO_TRACE_RECORD_MAX, 112);
    }

    #[test]
    fn test_action_word_geometry() {
        assert_eq!(BLK_TA_MASK, 0xFF);
        assert_eq!(BLK_TC_SHIFT, 16);
        assert_eq!(BLK_TC_MASK, 0xFFFF);
        // The TA byte and TC class bits do not overlap.
        assert_eq!(BLK_TA_MASK & (BLK_TC_MASK << BLK_TC_SHIFT), 0);
    }

    #[test]
    fn test_default_buffer_parameters() {
        assert_eq!(BLK_DEFAULT_BUF_SIZE, 512 * 1024);
        assert_eq!(BLK_DEFAULT_BUF_NR, 4);
        assert!(BLK_DEFAULT_BUF_SIZE.is_power_of_two());
        assert!(BLK_DEFAULT_BUF_NR.is_power_of_two());
        // Default total per-CPU buffer = 2 MiB.
        assert_eq!(BLK_DEFAULT_BUF_SIZE * BLK_DEFAULT_BUF_NR, 2 * 1024 * 1024);
    }

    #[test]
    fn test_minimum_buffer_parameters() {
        assert_eq!(BLK_MIN_BUF_SIZE, 16 * 1024);
        assert_eq!(BLK_MIN_BUF_NR, 2);
        assert!(BLK_MIN_BUF_SIZE < BLK_DEFAULT_BUF_SIZE);
        assert!(BLK_MIN_BUF_NR < BLK_DEFAULT_BUF_NR);
        // Defaults are 32x the minimum size.
        assert_eq!(BLK_DEFAULT_BUF_SIZE / BLK_MIN_BUF_SIZE, 32);
    }

    #[test]
    fn test_max_cpus_bound() {
        // 4096-CPU upper bound matches the kernel's NR_CPUS configuration.
        assert_eq!(BLK_MAX_CPUS, 4_096);
        assert!(BLK_MAX_CPUS.is_power_of_two());
    }
}
