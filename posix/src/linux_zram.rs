//! `<linux/zram_drv.h>` — ZRAM (compressed RAM disk) constants.
//!
//! ZRAM creates compressed block devices in RAM. Data written to
//! a zram device is compressed and stored in memory, providing
//! a fast swap device or tmpfs backing store that uses less RAM
//! than the uncompressed data would require.

// ---------------------------------------------------------------------------
// Compression algorithms
// ---------------------------------------------------------------------------

/// LZO compression (default).
pub const ZRAM_COMP_LZO: u8 = 0;
/// LZ4 compression (faster).
pub const ZRAM_COMP_LZ4: u8 = 1;
/// ZSTD compression (better ratio).
pub const ZRAM_COMP_ZSTD: u8 = 2;
/// LZO-RLE (Run Length Encoding variant).
pub const ZRAM_COMP_LZO_RLE: u8 = 3;
/// Deflate (zlib).
pub const ZRAM_COMP_DEFLATE: u8 = 4;
/// 842 compression (hardware-assisted on Power).
pub const ZRAM_COMP_842: u8 = 5;

// ---------------------------------------------------------------------------
// ZRAM states
// ---------------------------------------------------------------------------

/// Device not initialized.
pub const ZRAM_STATE_UNINIT: u8 = 0;
/// Device ready (disksize set).
pub const ZRAM_STATE_READY: u8 = 1;
/// Device active (in use).
pub const ZRAM_STATE_ACTIVE: u8 = 2;

// ---------------------------------------------------------------------------
// ZRAM flags (per-page)
// ---------------------------------------------------------------------------

/// Page is compressed.
pub const ZRAM_FLAG_COMPRESSED: u32 = 1 << 0;
/// Page is same-filled (single value repeated).
pub const ZRAM_FLAG_SAME: u32 = 1 << 1;
/// Page is written back to backing device.
pub const ZRAM_FLAG_WB: u32 = 1 << 2;
/// Page is huge (incompressible).
pub const ZRAM_FLAG_HUGE: u32 = 1 << 3;
/// Page is idle (candidate for writeback).
pub const ZRAM_FLAG_IDLE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Writeback modes (for backing device)
// ---------------------------------------------------------------------------

/// Writeback idle pages only.
pub const ZRAM_WB_IDLE: u32 = 1 << 0;
/// Writeback huge (incompressible) pages.
pub const ZRAM_WB_HUGE: u32 = 1 << 1;
/// Writeback huge and idle pages.
pub const ZRAM_WB_HUGE_IDLE: u32 = (1 << 0) | (1 << 1);

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of zram devices.
pub const ZRAM_MAX_DEVICES: u8 = 32;
/// Default maximum compression streams.
pub const ZRAM_DEFAULT_STREAMS: u8 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comp_algorithms_distinct() {
        let algs = [
            ZRAM_COMP_LZO, ZRAM_COMP_LZ4, ZRAM_COMP_ZSTD,
            ZRAM_COMP_LZO_RLE, ZRAM_COMP_DEFLATE, ZRAM_COMP_842,
        ];
        for i in 0..algs.len() {
            for j in (i + 1)..algs.len() {
                assert_ne!(algs[i], algs[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [ZRAM_STATE_UNINIT, ZRAM_STATE_READY, ZRAM_STATE_ACTIVE];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_page_flags_no_overlap() {
        let flags = [
            ZRAM_FLAG_COMPRESSED, ZRAM_FLAG_SAME,
            ZRAM_FLAG_WB, ZRAM_FLAG_HUGE, ZRAM_FLAG_IDLE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_wb_modes() {
        assert_eq!(ZRAM_WB_HUGE_IDLE, ZRAM_WB_IDLE | ZRAM_WB_HUGE);
    }
}
