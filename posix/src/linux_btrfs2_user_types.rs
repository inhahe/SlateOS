//! `<linux/btrfs.h>` (part 2) — Btrfs balance and scrub ioctls.
//!
//! Btrfs balance migrates blocks between profiles (single/raid/etc.)
//! while online. Scrub verifies checksums and repairs from mirrors.
//! Both are long-running and progress-reportable.

// ---------------------------------------------------------------------------
// Balance args flags
// ---------------------------------------------------------------------------

/// The data block group is selected for balance.
pub const BTRFS_BALANCE_DATA: u64 = 1 << 0;

/// The system block group is selected for balance.
pub const BTRFS_BALANCE_SYSTEM: u64 = 1 << 1;

/// The metadata block group is selected for balance.
pub const BTRFS_BALANCE_METADATA: u64 = 1 << 2;

/// Combined mask of all three.
pub const BTRFS_BALANCE_TYPE_MASK: u64 = BTRFS_BALANCE_DATA
    | BTRFS_BALANCE_SYSTEM
    | BTRFS_BALANCE_METADATA;

/// Force a chunk relocation even if it does not match filters.
pub const BTRFS_BALANCE_FORCE: u64 = 1 << 3;

/// Resume a previously paused balance.
pub const BTRFS_BALANCE_RESUME: u64 = 1 << 4;

// ---------------------------------------------------------------------------
// Balance state (`balance_args.state`)
// ---------------------------------------------------------------------------

pub const BTRFS_BALANCE_STATE_RUNNING: u64 = 1 << 0;
pub const BTRFS_BALANCE_STATE_PAUSE_REQ: u64 = 1 << 1;
pub const BTRFS_BALANCE_STATE_CANCEL_REQ: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Scrub control flags
// ---------------------------------------------------------------------------

/// Scrub read-only — do not fix found errors.
pub const BTRFS_SCRUB_READONLY: u64 = 1 << 0;

// ---------------------------------------------------------------------------
// Scrub progress field offsets (`btrfs_scrub_progress`, packed u64 fields)
// ---------------------------------------------------------------------------

pub const BTRFS_SCRUB_PROG_OFF_DATA_EXTENTS: usize = 0;
pub const BTRFS_SCRUB_PROG_OFF_TREE_EXTENTS: usize = 8;
pub const BTRFS_SCRUB_PROG_OFF_DATA_BYTES_SCRUBBED: usize = 16;
pub const BTRFS_SCRUB_PROG_OFF_TREE_BYTES_SCRUBBED: usize = 24;
pub const BTRFS_SCRUB_PROG_OFF_READ_ERRORS: usize = 32;
pub const BTRFS_SCRUB_PROG_OFF_CSUM_ERRORS: usize = 40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balance_type_flags_distinct_bits() {
        let f = [
            BTRFS_BALANCE_DATA,
            BTRFS_BALANCE_SYSTEM,
            BTRFS_BALANCE_METADATA,
        ];
        for &v in &f {
            assert!(v.is_power_of_two());
        }
        for (i, &a) in f.iter().enumerate() {
            for &b in &f[i + 1..] {
                assert_eq!(a & b, 0);
            }
        }
    }

    #[test]
    fn test_balance_type_mask_covers_all_three() {
        assert_eq!(
            BTRFS_BALANCE_TYPE_MASK,
            BTRFS_BALANCE_DATA | BTRFS_BALANCE_SYSTEM | BTRFS_BALANCE_METADATA
        );
        assert_eq!(BTRFS_BALANCE_TYPE_MASK.count_ones(), 3);
    }

    #[test]
    fn test_balance_control_flags_above_type_bits() {
        // FORCE / RESUME sit immediately above the three type bits.
        assert_eq!(BTRFS_BALANCE_FORCE, 1 << 3);
        assert_eq!(BTRFS_BALANCE_RESUME, 1 << 4);
        // Neither overlaps the type mask.
        assert_eq!(BTRFS_BALANCE_FORCE & BTRFS_BALANCE_TYPE_MASK, 0);
        assert_eq!(BTRFS_BALANCE_RESUME & BTRFS_BALANCE_TYPE_MASK, 0);
    }

    #[test]
    fn test_balance_states_dense_single_bits() {
        let s = [
            BTRFS_BALANCE_STATE_RUNNING,
            BTRFS_BALANCE_STATE_PAUSE_REQ,
            BTRFS_BALANCE_STATE_CANCEL_REQ,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v, 1 << i);
        }
    }

    #[test]
    fn test_scrub_readonly_single_bit() {
        assert!(BTRFS_SCRUB_READONLY.is_power_of_two());
        assert_eq!(BTRFS_SCRUB_READONLY, 1);
    }

    #[test]
    fn test_scrub_progress_layout_packed_u64s() {
        let o = [
            BTRFS_SCRUB_PROG_OFF_DATA_EXTENTS,
            BTRFS_SCRUB_PROG_OFF_TREE_EXTENTS,
            BTRFS_SCRUB_PROG_OFF_DATA_BYTES_SCRUBBED,
            BTRFS_SCRUB_PROG_OFF_TREE_BYTES_SCRUBBED,
            BTRFS_SCRUB_PROG_OFF_READ_ERRORS,
            BTRFS_SCRUB_PROG_OFF_CSUM_ERRORS,
        ];
        // Each field is exactly 8 bytes (u64) wide.
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v, i * 8);
        }
    }
}
