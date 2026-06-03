//! `<linux/dma-buf.h>` — DMA-BUF userspace sync-ioctl constants.
//!
//! DMA-BUF is the Linux kernel's cross-driver shared-buffer
//! mechanism. Userspace clients (Wayland compositors, V4L2 capture
//! pipelines, Vulkan/EGL importers) issue the sync ioctl to flush
//! CPU caches around explicit CPU access windows on imported
//! buffers. Constants below cover the sync flags and ioctl numbers
//! from the `dma-buf` uapi header.

// ---------------------------------------------------------------------------
// dma_buf_sync.flags
// ---------------------------------------------------------------------------

/// Access window will read from the buffer.
pub const DMA_BUF_SYNC_READ: u64 = 1 << 0;
/// Access window will write to the buffer.
pub const DMA_BUF_SYNC_WRITE: u64 = 2 << 0;
/// Combined read + write access window.
pub const DMA_BUF_SYNC_RW: u64 = DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE;
/// Begin a CPU access window (acquire).
pub const DMA_BUF_SYNC_START: u64 = 0 << 2;
/// End a CPU access window (release).
pub const DMA_BUF_SYNC_END: u64 = 1 << 2;

/// Bit-mask covering every valid flag (used to reject unknown bits).
pub const DMA_BUF_SYNC_VALID_FLAGS_MASK: u64 =
    DMA_BUF_SYNC_RW | DMA_BUF_SYNC_END;

// ---------------------------------------------------------------------------
// ioctl base / numbers (DMA_BUF_BASE = 'b', _IOW('b', 0, u64))
// ---------------------------------------------------------------------------

/// ioctl group letter used by the dma-buf uapi.
pub const DMA_BUF_BASE: u8 = b'b';

/// `DMA_BUF_IOCTL_SYNC` request number (32-bit, encoded `_IOW`).
///
/// Layout: dir=1 (write), size=8 (u64), type=`'b'`, nr=0.
pub const DMA_BUF_IOCTL_SYNC: u32 = 0x4008_6200;

/// `DMA_BUF_SET_NAME` ioctl number (raw, untyped variant).
pub const DMA_BUF_SET_NAME: u32 = 0x4008_6201;

/// Maximum buffer name length (`DMA_BUF_NAME_LEN`).
pub const DMA_BUF_NAME_LEN: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rw_is_or_of_r_and_w() {
        assert_eq!(DMA_BUF_SYNC_RW, DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE);
        assert_ne!(DMA_BUF_SYNC_READ, DMA_BUF_SYNC_WRITE);
    }

    #[test]
    fn test_start_end_distinct() {
        // START and END live in bit 2; START is zero, END is the set bit.
        assert_eq!(DMA_BUF_SYNC_START, 0);
        assert_ne!(DMA_BUF_SYNC_END, DMA_BUF_SYNC_START);
        assert!(DMA_BUF_SYNC_END.is_power_of_two());
    }

    #[test]
    fn test_valid_mask_covers_all_flags() {
        let all = DMA_BUF_SYNC_READ
            | DMA_BUF_SYNC_WRITE
            | DMA_BUF_SYNC_END
            | DMA_BUF_SYNC_START;
        assert_eq!(all & !DMA_BUF_SYNC_VALID_FLAGS_MASK, 0);
    }

    #[test]
    fn test_ioctl_numbers_distinct_and_share_group() {
        assert_ne!(DMA_BUF_IOCTL_SYNC, DMA_BUF_SET_NAME);
        // Byte 2 (bits 8..16) holds the ioctl group letter.
        assert_eq!((DMA_BUF_IOCTL_SYNC >> 8) & 0xff, u32::from(DMA_BUF_BASE));
        assert_eq!((DMA_BUF_SET_NAME >> 8) & 0xff, u32::from(DMA_BUF_BASE));
    }

    #[test]
    fn test_name_len_reasonable() {
        assert!(DMA_BUF_NAME_LEN >= 16);
        assert!(DMA_BUF_NAME_LEN <= 256);
    }
}
