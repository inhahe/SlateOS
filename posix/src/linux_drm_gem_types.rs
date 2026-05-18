//! `<drm/drm_gem.h>` — DRM GEM (Graphics Execution Manager) buffer constants.
//!
//! GEM manages GPU buffer objects — memory allocated for textures,
//! framebuffers, vertex buffers, command buffers, etc. GEM objects
//! are identified by handles (per-process) and can be shared between
//! processes via DMA-BUF (dma_buf_fd). The kernel tracks GEM objects
//! with reference counting; they're freed when all handles and
//! mappings are released.

// ---------------------------------------------------------------------------
// GEM domain flags (where the buffer is accessible)
// ---------------------------------------------------------------------------

/// CPU domain (accessible via mmap).
pub const GEM_DOMAIN_CPU: u32 = 0x01;
/// GTT domain (accessible via GPU's GART/IOMMU).
pub const GEM_DOMAIN_GTT: u32 = 0x02;
/// VRAM domain (GPU local memory, fastest for GPU).
pub const GEM_DOMAIN_VRAM: u32 = 0x04;

// ---------------------------------------------------------------------------
// GEM create flags
// ---------------------------------------------------------------------------

/// Buffer is mappable by CPU.
pub const GEM_CREATE_MAPPABLE: u32 = 0x01;
/// Buffer contents don't need to survive suspend.
pub const GEM_CREATE_VOLATILE: u32 = 0x02;
/// Buffer should be in contiguous physical memory.
pub const GEM_CREATE_CONTIGUOUS: u32 = 0x04;
/// Buffer is protected (hardware content protection / HDCP).
pub const GEM_CREATE_PROTECTED: u32 = 0x08;
/// Buffer is scanout-capable (can be used as framebuffer).
pub const GEM_CREATE_SCANOUT: u32 = 0x10;

// ---------------------------------------------------------------------------
// GEM mmap offset types
// ---------------------------------------------------------------------------

/// Write-back mmap (cached, CPU coherent).
pub const GEM_MMAP_WB: u32 = 0;
/// Write-combining mmap (suitable for GPU streaming writes).
pub const GEM_MMAP_WC: u32 = 1;
/// Uncached mmap (device memory).
pub const GEM_MMAP_UC: u32 = 2;

// ---------------------------------------------------------------------------
// GEM close/open operations
// ---------------------------------------------------------------------------

/// Open handle to an existing GEM object.
pub const GEM_OP_OPEN: u32 = 0;
/// Close a GEM handle (decrement reference).
pub const GEM_OP_CLOSE: u32 = 1;
/// Flink: create a global name for sharing.
pub const GEM_OP_FLINK: u32 = 2;

// ---------------------------------------------------------------------------
// Buffer object states
// ---------------------------------------------------------------------------

/// Buffer is idle (not referenced by GPU).
pub const GEM_BO_IDLE: u32 = 0;
/// Buffer is active (submitted to GPU, in flight).
pub const GEM_BO_ACTIVE: u32 = 1;
/// Buffer is purged (contents lost, volatile buffer reclaimed).
pub const GEM_BO_PURGED: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_flags_no_overlap() {
        let flags = [GEM_DOMAIN_CPU, GEM_DOMAIN_GTT, GEM_DOMAIN_VRAM];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_create_flags_no_overlap() {
        let flags = [
            GEM_CREATE_MAPPABLE, GEM_CREATE_VOLATILE,
            GEM_CREATE_CONTIGUOUS, GEM_CREATE_PROTECTED,
            GEM_CREATE_SCANOUT,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_mmap_types_distinct() {
        let types = [GEM_MMAP_WB, GEM_MMAP_WC, GEM_MMAP_UC];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_bo_states_distinct() {
        let states = [GEM_BO_IDLE, GEM_BO_ACTIVE, GEM_BO_PURGED];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
