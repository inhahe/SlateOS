//! `<linux/atm_zatm.h>` — ZeitNet ZATM private ATM ioctls.
//!
//! ZATM was the ZeitNet ZN122x ATM NIC driver shipped historically
//! in Linux's `drivers/atm/zatm`. Its private ioctls are still
//! reachable via `/dev/atm` and exercised by `atmtcp`-style test
//! tools. Constants below cover the driver-specific ioctl numbers,
//! the per-VCC scheduler classes, and the buffer-pool layout.

// ---------------------------------------------------------------------------
// ZATM-private ioctl base (driver letter 'a' in atm_ioc range)
// ---------------------------------------------------------------------------

/// Group-letter portion of every ZATM private ioctl.
pub const ZATM_IOCTL_LETTER: u8 = b'a';

// ---------------------------------------------------------------------------
// ZATM-private ioctl numbers (struct atmif_sioc.number)
// ---------------------------------------------------------------------------

/// Get pool information.
pub const ZATM_GETPOOL: u32 = 0x6160;
/// Get pool-config values.
pub const ZATM_GETPOOLZ: u32 = 0x6161;
/// Set pool configuration.
pub const ZATM_SETPOOL: u32 = 0x6162;

// ---------------------------------------------------------------------------
// VCC scheduler classes (struct zatm_pool_info.ratelim / class field)
// ---------------------------------------------------------------------------

/// Constant-bit-rate (CBR) service class.
pub const ZATM_CBR: u32 = 1;
/// Unspecified-bit-rate (UBR) service class.
pub const ZATM_UBR: u32 = 2;
/// Variable-bit-rate (VBR) service class.
pub const ZATM_VBR: u32 = 3;
/// Available-bit-rate (ABR) service class.
pub const ZATM_ABR: u32 = 4;

// ---------------------------------------------------------------------------
// Buffer-pool layout
// ---------------------------------------------------------------------------

/// Number of free-pool entries the ZN1221 maintains.
pub const ZATM_NUM_POOLS: u32 = 32;
/// Smallest valid pool entry — 32 bytes per chunk.
pub const ZATM_POOL_BYTES_MIN: u32 = 32;
/// Largest valid pool entry — 64 KiB per chunk.
pub const ZATM_POOL_BYTES_MAX: u32 = 65_536;

// ---------------------------------------------------------------------------
// AAL5 framing limits enforced by the hardware
// ---------------------------------------------------------------------------

/// Maximum AAL5 PDU size accepted by the ZATM hardware.
pub const ZATM_AAL5_MAX_PDU: u32 = 0xFFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_letter_is_a() {
        // ZATM shares the 'a' letter with the rest of the ATM ioctl
        // group; verify so a future rename to 'A' or similar is caught.
        assert_eq!(ZATM_IOCTL_LETTER, b'a');
    }

    #[test]
    fn test_pool_ioctls_distinct() {
        let i = [ZATM_GETPOOL, ZATM_GETPOOLZ, ZATM_SETPOOL];
        for x in 0..i.len() {
            for y in (x + 1)..i.len() {
                assert_ne!(i[x], i[y]);
            }
        }
        // All three share the same top-3-byte group.
        assert_eq!(ZATM_GETPOOL & 0xfff0, ZATM_GETPOOLZ & 0xfff0);
        assert_eq!(ZATM_GETPOOL & 0xfff0, ZATM_SETPOOL & 0xfff0);
    }

    #[test]
    fn test_service_classes_distinct() {
        let c = [ZATM_CBR, ZATM_UBR, ZATM_VBR, ZATM_ABR];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
    }

    #[test]
    fn test_pool_sizes_consistent() {
        assert!(ZATM_POOL_BYTES_MIN.is_power_of_two());
        assert!(ZATM_POOL_BYTES_MAX.is_power_of_two());
        assert!(ZATM_POOL_BYTES_MIN < ZATM_POOL_BYTES_MAX);
        assert!(ZATM_NUM_POOLS.is_power_of_two());
    }

    #[test]
    fn test_aal5_max_pdu_fits_in_u16() {
        // AAL5 length field is 16-bit; max PDU must equal u16::MAX.
        assert_eq!(ZATM_AAL5_MAX_PDU, 0xFFFF);
    }
}
