//! `<linux/dma-heap.h>` — `/dev/dma_heap/<name>` userspace ABI.
//!
//! dma-heap is the modern userspace allocator for shareable
//! DMA-buf objects (replacing ION). gralloc, hwcomposer, libcamera,
//! and gstreamer's v4l2 sink open `/dev/dma_heap/system` or
//! `/dev/dma_heap/cma` and issue `DMA_HEAP_IOCTL_ALLOC` to get a
//! pre-mapped dma-buf fd.

// ---------------------------------------------------------------------------
// ioctl group letter
// ---------------------------------------------------------------------------

/// Magic letter for dma-heap ioctls ('H').
pub const DMA_HEAP_IOC_MAGIC: u8 = b'H';

// ---------------------------------------------------------------------------
// ioctl number (single command)
// ---------------------------------------------------------------------------

/// `DMA_HEAP_IOCTL_ALLOC` — allocate a dma-buf from this heap.
pub const DMA_HEAP_IOCTL_ALLOC: u32 = 0xC018_4800;

// ---------------------------------------------------------------------------
// Heap allocation flags (struct dma_heap_allocation_data.heap_flags)
// ---------------------------------------------------------------------------

/// Allocate non-cached pages (write-combine on most arches).
pub const DMA_HEAP_VALID_HEAP_FLAGS: u64 = 0;

// ---------------------------------------------------------------------------
// Allocation fd flags (struct dma_heap_allocation_data.fd_flags)
// ---------------------------------------------------------------------------

/// Make the returned fd close-on-exec.
pub const O_CLOEXEC: u32 = 0x0008_0000;
/// Allow read access on the returned fd.
pub const O_RDONLY: u32 = 0x0000_0000;
/// Allow write access on the returned fd.
pub const O_WRONLY: u32 = 0x0000_0001;
/// Allow read+write access on the returned fd.
pub const O_RDWR: u32 = 0x0000_0002;
/// Access mode mask.
pub const O_ACCMODE: u32 = 0x0000_0003;

/// Mask of every fd_flags bit dma-heap accepts.
pub const DMA_HEAP_VALID_FD_FLAGS: u32 = O_CLOEXEC | O_ACCMODE;

// ---------------------------------------------------------------------------
// Well-known heap names (mountpoint suffixes under /dev/dma_heap/)
// ---------------------------------------------------------------------------

/// System heap (any-page allocator).
pub const DMA_HEAP_NAME_SYSTEM: &str = "system";
/// CMA heap (contiguous physical pages — for ISP/codec).
pub const DMA_HEAP_NAME_CMA: &str = "linux,cma";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_magic_h() {
        assert_eq!(DMA_HEAP_IOC_MAGIC, b'H');
        // Type byte 'H' in bits 8..15.
        assert_eq!((DMA_HEAP_IOCTL_ALLOC >> 8) & 0xff, b'H' as u32);
    }

    #[test]
    fn test_alloc_ioctl_distinct() {
        // There's only one ioctl, but its number must remain the
        // canonical 0xC018_4800 — userspace tools across drivers
        // hardcode this.
        assert_eq!(DMA_HEAP_IOCTL_ALLOC, 0xC018_4800);
    }

    #[test]
    fn test_fd_flags_mask_includes_accmode_and_cloexec() {
        // dma-heap only accepts CLOEXEC + RDONLY/WRONLY/RDWR in
        // fd_flags. The kernel rejects unknown bits with -EINVAL.
        assert_eq!(O_RDONLY, 0);
        assert_eq!(O_WRONLY, 1);
        assert_eq!(O_RDWR, 2);
        assert_eq!(O_ACCMODE, 3);
        assert!(O_CLOEXEC.is_power_of_two());
        assert_eq!(DMA_HEAP_VALID_FD_FLAGS, O_CLOEXEC | O_ACCMODE);
    }

    #[test]
    fn test_heap_flags_currently_zero() {
        // No heap_flags bits are defined yet — the valid mask is 0
        // and any nonzero bit must be rejected with -EINVAL.
        assert_eq!(DMA_HEAP_VALID_HEAP_FLAGS, 0);
    }

    #[test]
    fn test_heap_name_constants() {
        // These names match the kernel's device-tree binding strings.
        assert_eq!(DMA_HEAP_NAME_SYSTEM, "system");
        assert_eq!(DMA_HEAP_NAME_CMA, "linux,cma");
        assert_ne!(DMA_HEAP_NAME_SYSTEM, DMA_HEAP_NAME_CMA);
    }
}
