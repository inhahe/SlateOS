//! `<linux/regmap.h>` — Register map abstraction constants.
//!
//! Regmap provides a generic framework for accessing device registers
//! over various buses (I2C, SPI, MMIO, etc.). It handles caching,
//! byte order, register stride, and access validation, so drivers
//! don't need bus-specific register access code.

// ---------------------------------------------------------------------------
// Register access types
// ---------------------------------------------------------------------------

/// Register is readable.
pub const REGMAP_ACCESS_READ: u32 = 1 << 0;
/// Register is writable.
pub const REGMAP_ACCESS_WRITE: u32 = 1 << 1;
/// Register is volatile (not cached).
pub const REGMAP_ACCESS_VOLATILE: u32 = 1 << 2;
/// Register is precious (read has side effects).
pub const REGMAP_ACCESS_PRECIOUS: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Bus types
// ---------------------------------------------------------------------------

/// MMIO (memory-mapped I/O).
pub const REGMAP_BUS_MMIO: u8 = 0;
/// I2C bus.
pub const REGMAP_BUS_I2C: u8 = 1;
/// SPI bus.
pub const REGMAP_BUS_SPI: u8 = 2;
/// AC97 bus.
pub const REGMAP_BUS_AC97: u8 = 3;
/// SPMI bus (Qualcomm).
pub const REGMAP_BUS_SPMI: u8 = 4;
/// Slimbus.
pub const REGMAP_BUS_SLIMBUS: u8 = 5;
/// SDIO bus.
pub const REGMAP_BUS_SDIO: u8 = 6;

// ---------------------------------------------------------------------------
// Endianness
// ---------------------------------------------------------------------------

/// Native endian.
pub const REGMAP_ENDIAN_NATIVE: u8 = 0;
/// Big endian.
pub const REGMAP_ENDIAN_BIG: u8 = 1;
/// Little endian.
pub const REGMAP_ENDIAN_LITTLE: u8 = 2;

// ---------------------------------------------------------------------------
// Cache types
// ---------------------------------------------------------------------------

/// No cache.
pub const REGMAP_CACHE_NONE: u8 = 0;
/// Flat (array) cache.
pub const REGMAP_CACHE_FLAT: u8 = 1;
/// Rbtree cache (sparse registers).
pub const REGMAP_CACHE_RBTREE: u8 = 2;
/// Maple tree cache.
pub const REGMAP_CACHE_MAPLE: u8 = 3;

// ---------------------------------------------------------------------------
// Common register widths
// ---------------------------------------------------------------------------

/// 8-bit registers.
pub const REGMAP_REG_BITS_8: u8 = 8;
/// 16-bit registers.
pub const REGMAP_REG_BITS_16: u8 = 16;
/// 32-bit registers.
pub const REGMAP_REG_BITS_32: u8 = 32;

// ---------------------------------------------------------------------------
// IRQ types (for regmap-irq)
// ---------------------------------------------------------------------------

/// Level-triggered IRQ.
pub const REGMAP_IRQ_LEVEL: u8 = 0;
/// Edge-triggered IRQ.
pub const REGMAP_IRQ_EDGE: u8 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_flags_no_overlap() {
        let flags = [
            REGMAP_ACCESS_READ, REGMAP_ACCESS_WRITE,
            REGMAP_ACCESS_VOLATILE, REGMAP_ACCESS_PRECIOUS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_bus_types_distinct() {
        let buses = [
            REGMAP_BUS_MMIO, REGMAP_BUS_I2C, REGMAP_BUS_SPI,
            REGMAP_BUS_AC97, REGMAP_BUS_SPMI, REGMAP_BUS_SLIMBUS,
            REGMAP_BUS_SDIO,
        ];
        for i in 0..buses.len() {
            for j in (i + 1)..buses.len() {
                assert_ne!(buses[i], buses[j]);
            }
        }
    }

    #[test]
    fn test_endianness_distinct() {
        let endians = [REGMAP_ENDIAN_NATIVE, REGMAP_ENDIAN_BIG, REGMAP_ENDIAN_LITTLE];
        for i in 0..endians.len() {
            for j in (i + 1)..endians.len() {
                assert_ne!(endians[i], endians[j]);
            }
        }
    }

    #[test]
    fn test_cache_types_distinct() {
        let caches = [
            REGMAP_CACHE_NONE, REGMAP_CACHE_FLAT,
            REGMAP_CACHE_RBTREE, REGMAP_CACHE_MAPLE,
        ];
        for i in 0..caches.len() {
            for j in (i + 1)..caches.len() {
                assert_ne!(caches[i], caches[j]);
            }
        }
    }

    #[test]
    fn test_irq_types_distinct() {
        assert_ne!(REGMAP_IRQ_LEVEL, REGMAP_IRQ_EDGE);
    }
}
