//! `<linux/devmem.h>` — Device memory region constants.
//!
//! Device memory (devmem) provides userspace access to physical
//! memory regions, MMIO ranges, and device BARs via /dev/mem or
//! device-specific character devices. Access control is critical
//! for security (e.g., CONFIG_STRICT_DEVMEM).

// ---------------------------------------------------------------------------
// Memory region types
// ---------------------------------------------------------------------------

/// System RAM.
pub const DEVMEM_TYPE_RAM: u8 = 0;
/// MMIO (memory-mapped I/O).
pub const DEVMEM_TYPE_MMIO: u8 = 1;
/// PCI BAR region.
pub const DEVMEM_TYPE_PCI_BAR: u8 = 2;
/// ACPI tables.
pub const DEVMEM_TYPE_ACPI: u8 = 3;
/// Reserved/firmware.
pub const DEVMEM_TYPE_RESERVED: u8 = 4;
/// Persistent memory (PMEM).
pub const DEVMEM_TYPE_PMEM: u8 = 5;

// ---------------------------------------------------------------------------
// Access permissions
// ---------------------------------------------------------------------------

/// Readable.
pub const DEVMEM_PERM_READ: u32 = 1 << 0;
/// Writable.
pub const DEVMEM_PERM_WRITE: u32 = 1 << 1;
/// Executable.
pub const DEVMEM_PERM_EXEC: u32 = 1 << 2;
/// Cacheable.
pub const DEVMEM_PERM_CACHED: u32 = 1 << 3;
/// Write-combining.
pub const DEVMEM_PERM_WC: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// /dev/mem restrictions (CONFIG_STRICT_DEVMEM)
// ---------------------------------------------------------------------------

/// Unrestricted access (no strict devmem).
pub const DEVMEM_STRICT_NONE: u8 = 0;
/// Strict devmem (deny system RAM access).
pub const DEVMEM_STRICT_RAM: u8 = 1;
/// IO-strict (deny MMIO access too).
pub const DEVMEM_STRICT_IO: u8 = 2;

// ---------------------------------------------------------------------------
// Caching modes for MMIO regions
// ---------------------------------------------------------------------------

/// Uncacheable (UC).
pub const DEVMEM_CACHE_UC: u8 = 0;
/// Write-Combining (WC).
pub const DEVMEM_CACHE_WC: u8 = 1;
/// Write-Through (WT).
pub const DEVMEM_CACHE_WT: u8 = 2;
/// Write-Back (WB).
pub const DEVMEM_CACHE_WB: u8 = 3;
/// Uncacheable Minus (UC-).
pub const DEVMEM_CACHE_UC_MINUS: u8 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_types_distinct() {
        let types = [
            DEVMEM_TYPE_RAM, DEVMEM_TYPE_MMIO, DEVMEM_TYPE_PCI_BAR,
            DEVMEM_TYPE_ACPI, DEVMEM_TYPE_RESERVED, DEVMEM_TYPE_PMEM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_perms_no_overlap() {
        let perms = [
            DEVMEM_PERM_READ, DEVMEM_PERM_WRITE, DEVMEM_PERM_EXEC,
            DEVMEM_PERM_CACHED, DEVMEM_PERM_WC,
        ];
        for i in 0..perms.len() {
            for j in (i + 1)..perms.len() {
                assert_eq!(perms[i] & perms[j], 0);
            }
        }
    }

    #[test]
    fn test_strict_modes_distinct() {
        let modes = [DEVMEM_STRICT_NONE, DEVMEM_STRICT_RAM, DEVMEM_STRICT_IO];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_cache_modes_distinct() {
        let caches = [
            DEVMEM_CACHE_UC, DEVMEM_CACHE_WC, DEVMEM_CACHE_WT,
            DEVMEM_CACHE_WB, DEVMEM_CACHE_UC_MINUS,
        ];
        for i in 0..caches.len() {
            for j in (i + 1)..caches.len() {
                assert_ne!(caches[i], caches[j]);
            }
        }
    }
}
