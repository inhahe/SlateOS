//! `<linux/atm_zatm.h>` — ZeitNet ATM adapter (`zatm` driver) ioctls.
//!
//! ZeitNet ZN1221/ZN1225 cards used a special private ioctl surface
//! beyond the generic ATM API. While obsolete in production, the
//! constants remain in the kernel uapi and userspace tooling
//! (`atmtools`) still references them.

// ---------------------------------------------------------------------------
// Driver-name and sysfs identifiers
// ---------------------------------------------------------------------------

pub const ZATM_DRIVER_NAME: &str = "zatm";

// ---------------------------------------------------------------------------
// `zatm_pool_info.max_buf_size` defaults — pre-allocated free-buffer pools.
// ---------------------------------------------------------------------------

/// Small-buffer pool slot size (bytes).
pub const ZATM_POOL_SIZE_SMALL: u32 = 96;
/// Medium-buffer pool slot size.
pub const ZATM_POOL_SIZE_MED: u32 = 480;
/// Large-buffer pool slot size (one ATM AAL5 PDU rounded).
pub const ZATM_POOL_SIZE_LARGE: u32 = 1_536;

/// Maximum number of independent buffer pools.
pub const ZATM_NUM_POOLS: u32 = 8;

// ---------------------------------------------------------------------------
// Default cells-per-second (RFC 1483 LLC/SNAP)
// ---------------------------------------------------------------------------

pub const ZATM_CPS_NOMINAL_OC3: u32 = 353_207;
pub const ZATM_CPS_NOMINAL_E3: u32 = 80_000;

// ---------------------------------------------------------------------------
// Ioctl operation codes (`ZATM_GETPOOL`, `ZATM_SETPOOL`, `ZATM_GETPOOLZ`)
// ---------------------------------------------------------------------------

pub const ZATM_GETPOOL: u8 = 0xC0;
pub const ZATM_GETPOOLZ: u8 = 0xC1;
pub const ZATM_SETPOOL: u8 = 0xC2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_name() {
        assert_eq!(ZATM_DRIVER_NAME, "zatm");
    }

    #[test]
    fn test_pool_sizes_strictly_increasing() {
        assert!(ZATM_POOL_SIZE_SMALL < ZATM_POOL_SIZE_MED);
        assert!(ZATM_POOL_SIZE_MED < ZATM_POOL_SIZE_LARGE);
        // Large pool comfortably holds one AAL5 PDU split across cells.
        assert!(ZATM_POOL_SIZE_LARGE >= 1_500);
    }

    #[test]
    fn test_num_pools_power_of_two() {
        assert_eq!(ZATM_NUM_POOLS, 8);
        assert!(ZATM_NUM_POOLS.is_power_of_two());
    }

    #[test]
    fn test_oc3_cell_rate() {
        // OC-3 raw rate = 155.52 Mbps; usable ATM cell rate ≈ 353207 cps.
        assert_eq!(ZATM_CPS_NOMINAL_OC3, 353_207);
        // E3 is far slower than OC-3.
        assert!(ZATM_CPS_NOMINAL_E3 < ZATM_CPS_NOMINAL_OC3);
    }

    #[test]
    fn test_ioctl_codes_dense_c0_c2() {
        assert_eq!(ZATM_GETPOOL, 0xC0);
        assert_eq!(ZATM_GETPOOLZ, 0xC1);
        assert_eq!(ZATM_SETPOOL, 0xC2);
        assert_eq!(ZATM_GETPOOLZ - ZATM_GETPOOL, 1);
        assert_eq!(ZATM_SETPOOL - ZATM_GETPOOLZ, 1);
    }
}
