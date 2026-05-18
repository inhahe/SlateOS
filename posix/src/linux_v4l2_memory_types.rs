//! `<linux/videodev2.h>` (memory subset) — V4L2 memory and I/O method types.
//!
//! V4L2 supports several I/O methods for transferring video frames
//! between kernel and userspace. The memory type selects how buffers
//! are allocated and mapped: kernel-allocated and mmap'd, userspace-
//! allocated and registered, or DMA-BUF file descriptors for zero-
//! copy sharing between devices.

// ---------------------------------------------------------------------------
// Memory types (v4l2_memory)
// ---------------------------------------------------------------------------

/// Memory-mapped buffers (kernel allocates, user mmaps).
pub const V4L2_MEMORY_MMAP: u32 = 1;
/// User-pointer buffers (user allocates, kernel uses directly).
pub const V4L2_MEMORY_USERPTR: u32 = 2;
/// Video overlay (DMA to framebuffer).
pub const V4L2_MEMORY_OVERLAY: u32 = 3;
/// DMA-BUF file descriptors (zero-copy inter-device sharing).
pub const V4L2_MEMORY_DMABUF: u32 = 4;

// ---------------------------------------------------------------------------
// Streaming I/O ioctl codes (request numbers, not full ioctl encoding)
// ---------------------------------------------------------------------------

/// Request buffers: allocate or negotiate buffer count.
pub const VIDIOC_REQBUFS_NR: u32 = 8;
/// Query buffer: get mmap offset or userptr info.
pub const VIDIOC_QUERYBUF_NR: u32 = 9;
/// Queue buffer: submit to driver for filling/sending.
pub const VIDIOC_QBUF_NR: u32 = 15;
/// Dequeue buffer: retrieve filled/sent buffer.
pub const VIDIOC_DQBUF_NR: u32 = 17;
/// Start streaming.
pub const VIDIOC_STREAMON_NR: u32 = 18;
/// Stop streaming.
pub const VIDIOC_STREAMOFF_NR: u32 = 19;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_types_distinct() {
        let mems = [
            V4L2_MEMORY_MMAP, V4L2_MEMORY_USERPTR,
            V4L2_MEMORY_OVERLAY, V4L2_MEMORY_DMABUF,
        ];
        for i in 0..mems.len() {
            for j in (i + 1)..mems.len() {
                assert_ne!(mems[i], mems[j]);
            }
        }
    }

    #[test]
    fn test_memory_types_sequential() {
        assert_eq!(V4L2_MEMORY_MMAP, 1);
        assert_eq!(V4L2_MEMORY_USERPTR, 2);
        assert_eq!(V4L2_MEMORY_OVERLAY, 3);
        assert_eq!(V4L2_MEMORY_DMABUF, 4);
    }

    #[test]
    fn test_ioctl_nrs_distinct() {
        let nrs = [
            VIDIOC_REQBUFS_NR, VIDIOC_QUERYBUF_NR,
            VIDIOC_QBUF_NR, VIDIOC_DQBUF_NR,
            VIDIOC_STREAMON_NR, VIDIOC_STREAMOFF_NR,
        ];
        for i in 0..nrs.len() {
            for j in (i + 1)..nrs.len() {
                assert_ne!(nrs[i], nrs[j]);
            }
        }
    }

    #[test]
    fn test_stream_on_off_adjacent() {
        assert_eq!(VIDIOC_STREAMOFF_NR, VIDIOC_STREAMON_NR + 1);
    }

    #[test]
    fn test_all_nonzero() {
        assert_ne!(V4L2_MEMORY_MMAP, 0);
        assert_ne!(VIDIOC_REQBUFS_NR, 0);
    }
}
