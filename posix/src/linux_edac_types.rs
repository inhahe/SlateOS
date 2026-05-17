//! `<linux/edac.h>` — Error Detection And Correction (EDAC) constants.
//!
//! EDAC monitors memory controllers for ECC errors. It reports
//! correctable (CE) and uncorrectable (UE) memory errors via sysfs
//! (/sys/devices/system/edac/) with location information (DIMM,
//! channel, rank, bank, row, column). EDAC helps administrators
//! identify failing DIMMs before they cause data corruption.
//! Multiple memory controller drivers exist (amd64_edac, skx_edac,
//! ghes_edac for ACPI-reported errors, etc.).

// ---------------------------------------------------------------------------
// EDAC memory controller operation states
// ---------------------------------------------------------------------------

/// MC polling mode (driver polls for errors).
pub const EDAC_MC_POLL: u32 = 0;
/// MC interrupt mode (errors reported via interrupt/NMI).
pub const EDAC_MC_INTERRUPT: u32 = 1;

// ---------------------------------------------------------------------------
// EDAC error types
// ---------------------------------------------------------------------------

/// Correctable Error (single-bit ECC corrected).
pub const EDAC_ERROR_CE: u32 = 0;
/// Uncorrectable Error (multi-bit, data corrupted).
pub const EDAC_ERROR_UE: u32 = 1;

// ---------------------------------------------------------------------------
// EDAC error grain (error location granularity)
// ---------------------------------------------------------------------------

/// Error location unknown.
pub const EDAC_GRAIN_UNKNOWN: u32 = 0;
/// Error in a cache line (64 bytes typically).
pub const EDAC_GRAIN_CACHELINE: u32 = 1;
/// Error in a memory page.
pub const EDAC_GRAIN_PAGE: u32 = 2;
/// Error in an entire DIMM.
pub const EDAC_GRAIN_DIMM: u32 = 3;

// ---------------------------------------------------------------------------
// EDAC DIMM types
// ---------------------------------------------------------------------------

/// Unknown DIMM type.
pub const EDAC_DIMM_UNKNOWN: u32 = 0;
/// DDR2.
pub const EDAC_DIMM_DDR2: u32 = 1;
/// DDR3.
pub const EDAC_DIMM_DDR3: u32 = 2;
/// DDR4.
pub const EDAC_DIMM_DDR4: u32 = 3;
/// DDR5.
pub const EDAC_DIMM_DDR5: u32 = 4;
/// LPDDR4.
pub const EDAC_DIMM_LPDDR4: u32 = 5;
/// LPDDR5.
pub const EDAC_DIMM_LPDDR5: u32 = 6;
/// HBM (High Bandwidth Memory).
pub const EDAC_DIMM_HBM: u32 = 7;

// ---------------------------------------------------------------------------
// EDAC device types (non-memory EDAC-monitored components)
// ---------------------------------------------------------------------------

/// L1 cache.
pub const EDAC_DEVICE_L1_CACHE: u32 = 0;
/// L2 cache.
pub const EDAC_DEVICE_L2_CACHE: u32 = 1;
/// L3 cache.
pub const EDAC_DEVICE_L3_CACHE: u32 = 2;
/// CPU internal bus.
pub const EDAC_DEVICE_CPU_BUS: u32 = 3;

// ---------------------------------------------------------------------------
// EDAC sysfs attribute types
// ---------------------------------------------------------------------------

/// CE count per MC.
pub const EDAC_ATTR_CE_COUNT: u32 = 0;
/// UE count per MC.
pub const EDAC_ATTR_UE_COUNT: u32 = 1;
/// CE count without DIMM info.
pub const EDAC_ATTR_CE_NOINFO: u32 = 2;
/// UE count without DIMM info.
pub const EDAC_ATTR_UE_NOINFO: u32 = 3;
/// Reset counters.
pub const EDAC_ATTR_RESET: u32 = 4;
/// Polling interval (seconds).
pub const EDAC_ATTR_POLL_MSEC: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mc_modes_distinct() {
        assert_ne!(EDAC_MC_POLL, EDAC_MC_INTERRUPT);
    }

    #[test]
    fn test_error_types_distinct() {
        assert_ne!(EDAC_ERROR_CE, EDAC_ERROR_UE);
    }

    #[test]
    fn test_grain_distinct() {
        let grains = [
            EDAC_GRAIN_UNKNOWN, EDAC_GRAIN_CACHELINE,
            EDAC_GRAIN_PAGE, EDAC_GRAIN_DIMM,
        ];
        for i in 0..grains.len() {
            for j in (i + 1)..grains.len() {
                assert_ne!(grains[i], grains[j]);
            }
        }
    }

    #[test]
    fn test_dimm_types_distinct() {
        let dimms = [
            EDAC_DIMM_UNKNOWN, EDAC_DIMM_DDR2, EDAC_DIMM_DDR3,
            EDAC_DIMM_DDR4, EDAC_DIMM_DDR5, EDAC_DIMM_LPDDR4,
            EDAC_DIMM_LPDDR5, EDAC_DIMM_HBM,
        ];
        for i in 0..dimms.len() {
            for j in (i + 1)..dimms.len() {
                assert_ne!(dimms[i], dimms[j]);
            }
        }
    }

    #[test]
    fn test_device_types_distinct() {
        let devs = [
            EDAC_DEVICE_L1_CACHE, EDAC_DEVICE_L2_CACHE,
            EDAC_DEVICE_L3_CACHE, EDAC_DEVICE_CPU_BUS,
        ];
        for i in 0..devs.len() {
            for j in (i + 1)..devs.len() {
                assert_ne!(devs[i], devs[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            EDAC_ATTR_CE_COUNT, EDAC_ATTR_UE_COUNT,
            EDAC_ATTR_CE_NOINFO, EDAC_ATTR_UE_NOINFO,
            EDAC_ATTR_RESET, EDAC_ATTR_POLL_MSEC,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
