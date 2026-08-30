//! `<linux/cma.h>` — CMA (Contiguous Memory Allocator) constants.
//!
//! CMA provides large, physically-contiguous memory allocations for
//! devices that cannot use scatter-gather DMA (cameras, display
//! controllers, hardware codecs). It reserves memory regions at boot
//! that can be used by the page allocator when not needed for DMA,
//! then reclaimed for contiguous allocations on demand.

// ---------------------------------------------------------------------------
// CMA allocation flags
// ---------------------------------------------------------------------------

/// Allocate from default CMA area.
pub const CMA_ALLOC_DEFAULT: u32 = 0;
/// Do not warn on allocation failure.
pub const CMA_ALLOC_NOWARN: u32 = 1 << 0;
/// Allow retry with migration.
pub const CMA_ALLOC_RETRY: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// CMA area limits
// ---------------------------------------------------------------------------

/// Maximum number of CMA areas.
pub const CMA_MAX_AREAS: u32 = 64;
/// Default CMA alignment (page order).
pub const CMA_DEFAULT_ALIGN_ORDER: u32 = 0;
/// Maximum CMA alignment order.
pub const CMA_MAX_ALIGN_ORDER: u32 = 12;

// ---------------------------------------------------------------------------
// CMA region types (named areas)
// ---------------------------------------------------------------------------

/// Default CMA region (from cmdline or DT).
pub const CMA_REGION_DEFAULT: u8 = 0;
/// Device-specific CMA region.
pub const CMA_REGION_DEVICE: u8 = 1;
/// Secure CMA region (for TrustZone).
pub const CMA_REGION_SECURE: u8 = 2;

// ---------------------------------------------------------------------------
// Size constants
// ---------------------------------------------------------------------------

/// Minimum CMA area size (4 MiB).
pub const CMA_MIN_SIZE: u32 = 4 * 1024 * 1024;
/// Default CMA size if not specified (16 MiB on 32-bit, varies on 64-bit).
pub const CMA_DEFAULT_SIZE_32BIT: u32 = 16 * 1024 * 1024;
/// Typical default on 64-bit systems.
pub const CMA_DEFAULT_SIZE_64BIT: u32 = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_flags_no_overlap() {
        assert_eq!(CMA_ALLOC_NOWARN & CMA_ALLOC_RETRY, 0);
        assert!(CMA_ALLOC_NOWARN.is_power_of_two());
        assert!(CMA_ALLOC_RETRY.is_power_of_two());
    }

    #[test]
    fn test_limits() {
        assert!(CMA_MAX_AREAS > 0);
        assert!(CMA_DEFAULT_ALIGN_ORDER < CMA_MAX_ALIGN_ORDER);
    }

    #[test]
    fn test_region_types_distinct() {
        let types = [CMA_REGION_DEFAULT, CMA_REGION_DEVICE, CMA_REGION_SECURE];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_size_ordering() {
        assert!(CMA_MIN_SIZE < CMA_DEFAULT_SIZE_32BIT);
        assert!(CMA_DEFAULT_SIZE_32BIT < CMA_DEFAULT_SIZE_64BIT);
    }
}
