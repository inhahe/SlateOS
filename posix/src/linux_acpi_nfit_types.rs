//! `<acpi/nfit.h>` — ACPI NVDIMM Firmware Interface Table (NFIT) constants.
//!
//! NFIT describes NVDIMM (persistent memory) topology and
//! capabilities to the OS. It maps physical NVDIMM regions to
//! system physical addresses, describes interleave sets (stripes
//! across DIMMs), and provides ARS (Address Range Scrub) for
//! detecting media errors. The libnvdimm subsystem consumes NFIT
//! to create namespace and region objects exposed via /dev/pmemN
//! and /dev/daxN.M devices.

// ---------------------------------------------------------------------------
// NFIT structure types (subtable types in the NFIT ACPI table)
// ---------------------------------------------------------------------------

/// System Physical Address Range Descriptor.
pub const NFIT_TYPE_SPA_RANGE: u32 = 0;
/// NVDIMM Region Mapping Descriptor.
pub const NFIT_TYPE_REGION_MAPPING: u32 = 1;
/// Interleave Descriptor.
pub const NFIT_TYPE_INTERLEAVE: u32 = 2;
/// SMBIOS Management Information.
pub const NFIT_TYPE_SMBIOS: u32 = 3;
/// NVDIMM Control Region Descriptor.
pub const NFIT_TYPE_CONTROL_REGION: u32 = 4;
/// NVDIMM Block Data Window Region.
pub const NFIT_TYPE_DATA_REGION: u32 = 5;
/// Flush Hint Address.
pub const NFIT_TYPE_FLUSH_HINT: u32 = 6;
/// Platform Capabilities.
pub const NFIT_TYPE_CAPABILITIES: u32 = 7;

// ---------------------------------------------------------------------------
// NFIT SPA range types (what kind of memory the range provides)
// ---------------------------------------------------------------------------

/// Volatile memory (standard DRAM-like).
pub const NFIT_SPA_VOLATILE: u32 = 0;
/// Persistent memory (byte-addressable PMEM).
pub const NFIT_SPA_PM: u32 = 1;
/// Control region (NVDIMM command/status registers).
pub const NFIT_SPA_CONTROL: u32 = 2;
/// Block data window (block-mode access).
pub const NFIT_SPA_BLK_DATA: u32 = 3;
/// Volatile virtual disk.
pub const NFIT_SPA_VOLATILE_VD: u32 = 4;
/// Persistent virtual disk.
pub const NFIT_SPA_PM_VD: u32 = 5;

// ---------------------------------------------------------------------------
// NFIT ARS (Address Range Scrub) status
// ---------------------------------------------------------------------------

/// ARS not started.
pub const NFIT_ARS_NOT_STARTED: u32 = 0;
/// ARS in progress.
pub const NFIT_ARS_IN_PROGRESS: u32 = 1;
/// ARS complete.
pub const NFIT_ARS_COMPLETE: u32 = 2;
/// ARS completed with errors found.
pub const NFIT_ARS_COMPLETE_WITH_ERRORS: u32 = 3;

// ---------------------------------------------------------------------------
// NFIT NVDIMM state flags
// ---------------------------------------------------------------------------

/// NVDIMM is not armed (not ready for persistence).
pub const NFIT_MEM_NOT_ARMED: u32 = 1 << 0;
/// NVDIMM health event occurred.
pub const NFIT_MEM_HEALTH_EVENT: u32 = 1 << 1;
/// NVDIMM health is critical.
pub const NFIT_MEM_HEALTH_CRITICAL: u32 = 1 << 2;
/// NVDIMM mapped (SPA range assigned).
pub const NFIT_MEM_MAPPED: u32 = 1 << 3;
/// NVDIMM flush failed.
pub const NFIT_MEM_FLUSH_FAIL: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// NFIT platform capabilities
// ---------------------------------------------------------------------------

/// CPU cache flush to NVDIMM on power loss.
pub const NFIT_CAP_CACHE_FLUSH: u32 = 1 << 0;
/// Memory controller flush to NVDIMM on power loss.
pub const NFIT_CAP_MEM_FLUSH: u32 = 1 << 1;
/// Hardware mirroring support.
pub const NFIT_CAP_MIRROR: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_structure_types_distinct() {
        let types = [
            NFIT_TYPE_SPA_RANGE,
            NFIT_TYPE_REGION_MAPPING,
            NFIT_TYPE_INTERLEAVE,
            NFIT_TYPE_SMBIOS,
            NFIT_TYPE_CONTROL_REGION,
            NFIT_TYPE_DATA_REGION,
            NFIT_TYPE_FLUSH_HINT,
            NFIT_TYPE_CAPABILITIES,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_spa_types_distinct() {
        let spas = [
            NFIT_SPA_VOLATILE,
            NFIT_SPA_PM,
            NFIT_SPA_CONTROL,
            NFIT_SPA_BLK_DATA,
            NFIT_SPA_VOLATILE_VD,
            NFIT_SPA_PM_VD,
        ];
        for i in 0..spas.len() {
            for j in (i + 1)..spas.len() {
                assert_ne!(spas[i], spas[j]);
            }
        }
    }

    #[test]
    fn test_ars_states_distinct() {
        let states = [
            NFIT_ARS_NOT_STARTED,
            NFIT_ARS_IN_PROGRESS,
            NFIT_ARS_COMPLETE,
            NFIT_ARS_COMPLETE_WITH_ERRORS,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_mem_flags_no_overlap() {
        let flags = [
            NFIT_MEM_NOT_ARMED,
            NFIT_MEM_HEALTH_EVENT,
            NFIT_MEM_HEALTH_CRITICAL,
            NFIT_MEM_MAPPED,
            NFIT_MEM_FLUSH_FAIL,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_capabilities_no_overlap() {
        let caps = [NFIT_CAP_CACHE_FLUSH, NFIT_CAP_MEM_FLUSH, NFIT_CAP_MIRROR];
        for i in 0..caps.len() {
            assert!(caps[i].is_power_of_two());
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }
}
