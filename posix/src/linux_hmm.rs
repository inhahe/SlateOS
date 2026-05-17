//! `<linux/hmm.h>` — HMM (Heterogeneous Memory Management) constants.
//!
//! HMM allows devices (GPUs, FPGAs, accelerators) to transparently
//! access process virtual memory through page table mirroring. The
//! device's MMU mirrors the CPU page tables, allowing unified virtual
//! addressing between CPU and accelerator without explicit memory
//! copies or pinning.

// ---------------------------------------------------------------------------
// HMM page flags (returned by hmm_range_fault)
// ---------------------------------------------------------------------------

/// Page is valid (mapped).
pub const HMM_PFN_VALID: u64 = 1 << 0;
/// Page is writable.
pub const HMM_PFN_WRITE: u64 = 1 << 1;
/// Page is a device private page.
pub const HMM_PFN_DEVICE_PRIVATE: u64 = 1 << 2;
/// Error accessing this page.
pub const HMM_PFN_ERROR: u64 = 1 << 3;

// ---------------------------------------------------------------------------
// HMM range fault flags (input)
// ---------------------------------------------------------------------------

/// Fault for read access.
pub const HMM_FAULT_READ: u32 = 0;
/// Fault for write access.
pub const HMM_FAULT_WRITE: u32 = 1;
/// Snapshot only (no actual fault, just read current state).
pub const HMM_FAULT_SNAPSHOT: u32 = 2;

// ---------------------------------------------------------------------------
// Device memory types
// ---------------------------------------------------------------------------

/// Device private memory (not accessible by CPU).
pub const HMM_DEV_PRIVATE: u8 = 0;
/// Device coherent memory (CPU-accessible device memory).
pub const HMM_DEV_COHERENT: u8 = 1;

// ---------------------------------------------------------------------------
// Migration direction
// ---------------------------------------------------------------------------

/// Migrate from CPU to device.
pub const HMM_MIGRATE_TO_DEVICE: u8 = 0;
/// Migrate from device to CPU (system RAM).
pub const HMM_MIGRATE_TO_SYSTEM: u8 = 1;

// ---------------------------------------------------------------------------
// PFN mask and shift
// ---------------------------------------------------------------------------

/// PFN mask (bits containing the actual page frame number).
pub const HMM_PFN_SHIFT: u32 = 4;
/// Flags mask (bottom 4 bits).
pub const HMM_PFN_FLAGS_MASK: u64 = 0xF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pfn_flags_no_overlap() {
        let flags = [HMM_PFN_VALID, HMM_PFN_WRITE, HMM_PFN_DEVICE_PRIVATE, HMM_PFN_ERROR];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_fault_types_distinct() {
        let types = [HMM_FAULT_READ, HMM_FAULT_WRITE, HMM_FAULT_SNAPSHOT];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_dev_memory_types_distinct() {
        assert_ne!(HMM_DEV_PRIVATE, HMM_DEV_COHERENT);
    }

    #[test]
    fn test_migration_directions_distinct() {
        assert_ne!(HMM_MIGRATE_TO_DEVICE, HMM_MIGRATE_TO_SYSTEM);
    }

    #[test]
    fn test_pfn_encoding() {
        // All flag bits should be within the flags mask
        assert_eq!(HMM_PFN_VALID & HMM_PFN_FLAGS_MASK, HMM_PFN_VALID);
        assert_eq!(HMM_PFN_ERROR & HMM_PFN_FLAGS_MASK, HMM_PFN_ERROR);
        assert_eq!(HMM_PFN_SHIFT, 4);
    }
}
