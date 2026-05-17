//! `<linux/hmm.h>` — Heterogeneous Memory Management extended constants.
//!
//! HMM allows devices (GPUs, FPGAs, accelerators) to mirror a process's
//! virtual address space. Pages can be migrated between system RAM and
//! device memory transparently, using the same virtual addresses in
//! both CPU and device page tables. HMM hooks into the MMU notifier
//! system to keep CPU and device page tables synchronized, and provides
//! page fault forwarding so the device can trigger page-ins from swap
//! or file-backed mappings.

// ---------------------------------------------------------------------------
// HMM page flags (returned by hmm_range_fault)
// ---------------------------------------------------------------------------

/// Page is valid and mapped.
pub const HMM_PFN_VALID: u64 = 1 << 0;
/// Page is writable.
pub const HMM_PFN_WRITE: u64 = 1 << 1;
/// Page is in device private memory.
pub const HMM_PFN_DEVICE_PRIVATE: u64 = 1 << 2;
/// Error occurred resolving this page.
pub const HMM_PFN_ERROR: u64 = 1 << 3;
/// Page requires special handling (ZONE_DEVICE).
pub const HMM_PFN_SPECIAL: u64 = 1 << 4;

// ---------------------------------------------------------------------------
// HMM range fault flags (input to hmm_range_fault)
// ---------------------------------------------------------------------------

/// Fault for read access.
pub const HMM_FAULT_READ: u32 = 0x01;
/// Fault for write access (implies read).
pub const HMM_FAULT_WRITE: u32 = 0x02;
/// Snapshot only (don't actually fault in pages).
pub const HMM_FAULT_SNAPSHOT: u32 = 0x04;

// ---------------------------------------------------------------------------
// HMM migration actions
// ---------------------------------------------------------------------------

/// Migrate page from system RAM to device memory.
pub const HMM_MIGRATE_TO_DEVICE: u32 = 0;
/// Migrate page from device memory to system RAM.
pub const HMM_MIGRATE_TO_RAM: u32 = 1;

// ---------------------------------------------------------------------------
// HMM device memory types
// ---------------------------------------------------------------------------

/// Private device memory (only device can access).
pub const HMM_DEVMEM_PRIVATE: u32 = 0;
/// Coherent device memory (CPU and device can access).
pub const HMM_DEVMEM_COHERENT: u32 = 1;

// ---------------------------------------------------------------------------
// HMM limits
// ---------------------------------------------------------------------------

/// Maximum number of pages per HMM range operation.
pub const HMM_RANGE_MAX_PAGES: u32 = 1 << 20; // 1M pages
/// PFN shift (number of flag bits in the low bits of hmm_pfn).
pub const HMM_PFN_SHIFT: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pfn_flags_no_overlap() {
        let flags: [u64; 5] = [
            HMM_PFN_VALID, HMM_PFN_WRITE, HMM_PFN_DEVICE_PRIVATE,
            HMM_PFN_ERROR, HMM_PFN_SPECIAL,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_fault_flags_no_overlap() {
        let flags = [HMM_FAULT_READ, HMM_FAULT_WRITE, HMM_FAULT_SNAPSHOT];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_migration_actions_distinct() {
        assert_ne!(HMM_MIGRATE_TO_DEVICE, HMM_MIGRATE_TO_RAM);
    }

    #[test]
    fn test_devmem_types_distinct() {
        assert_ne!(HMM_DEVMEM_PRIVATE, HMM_DEVMEM_COHERENT);
    }

    #[test]
    fn test_limits() {
        assert!(HMM_RANGE_MAX_PAGES > 0);
        assert!(HMM_PFN_SHIFT > 0);
    }
}
