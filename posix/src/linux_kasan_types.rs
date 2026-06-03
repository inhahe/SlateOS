//! `<linux/kasan.h>` — Kernel Address Sanitizer constants.
//!
//! Constants for KASAN — the kernel address sanitizer that detects
//! out-of-bounds and use-after-free bugs via a shadow-memory mapping.
//! Userspace KASAN test harnesses and `dmesg` parsers consume these.

// ---------------------------------------------------------------------------
// Shadow-memory bookkeeping
// ---------------------------------------------------------------------------

/// Each KASAN shadow byte tracks this many bytes of real memory.
pub const KASAN_GRANULE_SIZE: u32 = 8;
/// log2(KASAN_GRANULE_SIZE).
pub const KASAN_SHADOW_SCALE_SHIFT: u32 = 3;

// ---------------------------------------------------------------------------
// Shadow byte poison values (in the shadow map, not in real memory)
// ---------------------------------------------------------------------------

/// Region is accessible.
pub const KASAN_SHADOW_BYTE_ZERO: u32 = 0x00;
/// Trailing N bytes of the granule are inaccessible (1..=7).
pub const KASAN_SHADOW_BYTE_PARTIAL_BASE: u32 = 0x01;
/// Region in the heap "freed" state.
pub const KASAN_FREE_PAGE: u32 = 0xFF;
/// Allocator-internal page poison.
pub const KASAN_PAGE_REDZONE: u32 = 0xFE;
/// Heap red-zone — past-end OOB protection.
pub const KASAN_KMALLOC_REDZONE: u32 = 0xFC;
/// Freed heap object — use-after-free protection.
pub const KASAN_KMALLOC_FREE: u32 = 0xFB;
/// Stack-frame red zone (left).
pub const KASAN_STACK_LEFT: u32 = 0xF1;
/// Stack-frame red zone (middle, padding).
pub const KASAN_STACK_MID: u32 = 0xF2;
/// Stack-frame red zone (right).
pub const KASAN_STACK_RIGHT: u32 = 0xF3;
/// Variable went out of scope but the spill slot is still in use.
pub const KASAN_USE_AFTER_SCOPE: u32 = 0xF8;

// ---------------------------------------------------------------------------
// KASAN mode codes (CONFIG_KASAN_GENERIC vs HW_TAGS vs SW_TAGS)
// ---------------------------------------------------------------------------

/// Generic KASAN.
pub const KASAN_MODE_GENERIC: u32 = 0;
/// Software tag-based KASAN.
pub const KASAN_MODE_SW_TAGS: u32 = 1;
/// Hardware tag-based KASAN (MTE on ARM).
pub const KASAN_MODE_HW_TAGS: u32 = 2;

// ---------------------------------------------------------------------------
// Report severity / error categories
// ---------------------------------------------------------------------------

/// Out-of-bounds (slab).
pub const KASAN_ERR_SLAB_OOB: u32 = 0;
/// Use-after-free (slab).
pub const KASAN_ERR_SLAB_UAF: u32 = 1;
/// Out-of-bounds (page allocator).
pub const KASAN_ERR_PAGE_OOB: u32 = 2;
/// Use-after-free (page allocator).
pub const KASAN_ERR_PAGE_UAF: u32 = 3;
/// Out-of-bounds (stack).
pub const KASAN_ERR_STACK_OOB: u32 = 4;
/// Use-after-scope (stack).
pub const KASAN_ERR_STACK_UAS: u32 = 5;
/// Out-of-bounds (global).
pub const KASAN_ERR_GLOBAL_OOB: u32 = 6;
/// Invalid free.
pub const KASAN_ERR_INVALID_FREE: u32 = 7;
/// Double free.
pub const KASAN_ERR_DOUBLE_FREE: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_granule_relationship() {
        // KASAN granule and shift must agree: 1 << shift == granule.
        assert_eq!(1u32 << KASAN_SHADOW_SCALE_SHIFT, KASAN_GRANULE_SIZE);
    }

    #[test]
    fn test_poison_bytes_distinct() {
        let bytes = [
            KASAN_FREE_PAGE,
            KASAN_PAGE_REDZONE,
            KASAN_KMALLOC_REDZONE,
            KASAN_KMALLOC_FREE,
            KASAN_STACK_LEFT,
            KASAN_STACK_MID,
            KASAN_STACK_RIGHT,
            KASAN_USE_AFTER_SCOPE,
        ];
        for i in 0..bytes.len() {
            for j in (i + 1)..bytes.len() {
                assert_ne!(bytes[i], bytes[j]);
            }
        }
        // All real poison values live in the high half (>= 0x80) so
        // they never collide with "partially accessible" markers
        // (0x01..=0x07).
        for &b in &bytes {
            assert!(b >= 0x80);
        }
    }

    #[test]
    fn test_modes_distinct() {
        let modes = [KASAN_MODE_GENERIC, KASAN_MODE_SW_TAGS, KASAN_MODE_HW_TAGS];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_error_categories_distinct() {
        let errs = [
            KASAN_ERR_SLAB_OOB,
            KASAN_ERR_SLAB_UAF,
            KASAN_ERR_PAGE_OOB,
            KASAN_ERR_PAGE_UAF,
            KASAN_ERR_STACK_OOB,
            KASAN_ERR_STACK_UAS,
            KASAN_ERR_GLOBAL_OOB,
            KASAN_ERR_INVALID_FREE,
            KASAN_ERR_DOUBLE_FREE,
        ];
        for i in 0..errs.len() {
            for j in (i + 1)..errs.len() {
                assert_ne!(errs[i], errs[j]);
            }
        }
    }

    #[test]
    fn test_partial_base_in_low_range() {
        // Partial-accessibility markers occupy [1..=GRANULE-1]; the base
        // must be 1 and stay below GRANULE_SIZE.
        assert_eq!(KASAN_SHADOW_BYTE_PARTIAL_BASE, 1);
        assert!(KASAN_SHADOW_BYTE_PARTIAL_BASE < KASAN_GRANULE_SIZE);
        assert_eq!(KASAN_SHADOW_BYTE_ZERO, 0);
    }
}
