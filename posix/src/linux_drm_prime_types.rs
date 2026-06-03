//! `<drm/drm.h>` — DRM PRIME (DMA-BUF sharing) constants.
//!
//! PRIME enables zero-copy buffer sharing between DRM devices and
//! between GPU and other subsystems (V4L2, display, codec). It uses
//! DMA-BUF file descriptors as handles for GPU memory that can be
//! imported/exported across device boundaries.

// ---------------------------------------------------------------------------
// PRIME handle-to-fd flags
// ---------------------------------------------------------------------------

/// Close-on-exec for the exported DMA-BUF fd.
pub const DRM_PRIME_FD_CLOEXEC: u32 = 0x01;
/// Read access for the exported buffer.
pub const DRM_PRIME_FD_READ: u32 = 0x02;
/// Write access for the exported buffer.
pub const DRM_PRIME_FD_WRITE: u32 = 0x04;

// ---------------------------------------------------------------------------
// DMA-BUF sync flags (for DMA_BUF_IOCTL_SYNC)
// ---------------------------------------------------------------------------

/// Begin CPU access (start sync).
pub const DMA_BUF_SYNC_START: u64 = 0;
/// End CPU access (end sync).
pub const DMA_BUF_SYNC_END: u64 = 1 << 2;
/// CPU read access.
pub const DMA_BUF_SYNC_READ: u64 = 1 << 0;
/// CPU write access.
pub const DMA_BUF_SYNC_WRITE: u64 = 1 << 1;
/// CPU read+write access.
pub const DMA_BUF_SYNC_RW: u64 = (1 << 0) | (1 << 1);

// ---------------------------------------------------------------------------
// GEM (Graphics Execution Manager) common constants
// ---------------------------------------------------------------------------

/// Maximum number of GEM handles per open file.
pub const DRM_GEM_MAX_HANDLES: u32 = 1024;

/// GEM domain: CPU (coherent with CPU cache).
pub const DRM_GEM_DOMAIN_CPU: u32 = 1 << 0;
/// GEM domain: GTT (GPU-visible via GART/GTT mapping).
pub const DRM_GEM_DOMAIN_GTT: u32 = 1 << 1;
/// GEM domain: VRAM (local video memory).
pub const DRM_GEM_DOMAIN_VRAM: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Buffer object placement flags (common across drivers)
// ---------------------------------------------------------------------------

/// Prefer CPU-accessible memory.
pub const DRM_BO_FLAG_CPU_ACCESSIBLE: u32 = 1 << 0;
/// Buffer is scanout target (may need contiguous).
pub const DRM_BO_FLAG_SCANOUT: u32 = 1 << 1;
/// Buffer is a cursor image.
pub const DRM_BO_FLAG_CURSOR: u32 = 1 << 2;
/// Buffer for rendering (GPU source/dest).
pub const DRM_BO_FLAG_RENDER: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prime_fd_flags_distinct() {
        let flags = [DRM_PRIME_FD_CLOEXEC, DRM_PRIME_FD_READ, DRM_PRIME_FD_WRITE];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_dma_buf_sync_rw() {
        assert_eq!(DMA_BUF_SYNC_RW, DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE);
    }

    #[test]
    fn test_gem_domains_no_overlap() {
        let doms = [DRM_GEM_DOMAIN_CPU, DRM_GEM_DOMAIN_GTT, DRM_GEM_DOMAIN_VRAM];
        for i in 0..doms.len() {
            assert!(doms[i].is_power_of_two());
            for j in (i + 1)..doms.len() {
                assert_eq!(doms[i] & doms[j], 0);
            }
        }
    }

    #[test]
    fn test_bo_flags_no_overlap() {
        let flags = [
            DRM_BO_FLAG_CPU_ACCESSIBLE,
            DRM_BO_FLAG_SCANOUT,
            DRM_BO_FLAG_CURSOR,
            DRM_BO_FLAG_RENDER,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
