//! `<linux/dma-buf.h>` — DMA buffer sharing ioctls.
//!
//! DMA-BUF is the Linux kernel framework for sharing buffers between
//! drivers and userspace (e.g., GPU ↔ display, GPU ↔ video codec).
//! The ioctl interface is used for synchronization and querying.

// ---------------------------------------------------------------------------
// DMA-BUF ioctl commands
// ---------------------------------------------------------------------------

/// Sync the buffer for CPU access.
pub const DMA_BUF_IOCTL_SYNC: u64 = 0x40086200;
/// Set name for the buffer.
pub const DMA_BUF_SET_NAME: u64 = 0x40086201;
/// Set name (variant B).
pub const DMA_BUF_SET_NAME_B: u64 = 0x40046201;

// ---------------------------------------------------------------------------
// DMA-BUF sync flags
// ---------------------------------------------------------------------------

/// Sync for reading.
pub const DMA_BUF_SYNC_READ: u64 = 1 << 0;
/// Sync for writing.
pub const DMA_BUF_SYNC_WRITE: u64 = 1 << 1;
/// Sync for both read and write.
pub const DMA_BUF_SYNC_RW: u64 = DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE;
/// Begin sync operation.
pub const DMA_BUF_SYNC_START: u64 = 0 << 2;
/// End sync operation.
pub const DMA_BUF_SYNC_END: u64 = 1 << 2;
/// Valid sync flags mask.
pub const DMA_BUF_SYNC_VALID_FLAGS_MASK: u64 =
    DMA_BUF_SYNC_RW | DMA_BUF_SYNC_END;

// ---------------------------------------------------------------------------
// DMA-BUF sync struct
// ---------------------------------------------------------------------------

/// DMA-BUF sync parameters.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DmaBufSync {
    /// Sync flags.
    pub flags: u64,
}

// ---------------------------------------------------------------------------
// DMA heap ioctl (related)
// ---------------------------------------------------------------------------

/// DMA heap allocation ioctl.
pub const DMA_HEAP_IOCTL_ALLOC: u64 = 0xC0186800;

/// DMA heap allocation parameters.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DmaHeapAllocationData {
    /// Requested size (in bytes).
    pub len: u64,
    /// Returned file descriptor.
    pub fd: u32,
    /// Heap-specific flags.
    pub fd_flags: u32,
    /// Flags for the allocation.
    pub heap_flags: u64,
}

impl DmaHeapAllocationData {
    /// Create a zeroed allocation data.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_flags() {
        assert_eq!(DMA_BUF_SYNC_READ, 1);
        assert_eq!(DMA_BUF_SYNC_WRITE, 2);
        assert_eq!(DMA_BUF_SYNC_RW, 3);
        assert_eq!(DMA_BUF_SYNC_START, 0);
        assert_eq!(DMA_BUF_SYNC_END, 4);
    }

    #[test]
    fn test_sync_struct_size() {
        assert_eq!(core::mem::size_of::<DmaBufSync>(), 8);
    }

    #[test]
    fn test_heap_allocation_size() {
        assert_eq!(core::mem::size_of::<DmaHeapAllocationData>(), 24);
    }

    #[test]
    fn test_heap_allocation_zeroed() {
        let data = DmaHeapAllocationData::zeroed();
        assert_eq!(data.len, 0);
        assert_eq!(data.fd, 0);
        assert_eq!(data.fd_flags, 0);
        assert_eq!(data.heap_flags, 0);
    }

    #[test]
    fn test_valid_flags_mask() {
        assert_eq!(DMA_BUF_SYNC_VALID_FLAGS_MASK, DMA_BUF_SYNC_RW | DMA_BUF_SYNC_END);
    }
}
