//! `<linux/clk-provider.h>` — Clock provider operations and divider tables.
//!
//! Clock provider drivers register `struct clk_hw` instances with the
//! Common Clock Framework. Each clock kind (divider, mux, gate, mux,
//! fractional, multiplier) has its own flag set governing rounding,
//! parent locking, and zero-divider semantics.

// ---------------------------------------------------------------------------
// clk_divider flags
// ---------------------------------------------------------------------------

/// Divider register value is encoded as 1<<value (power-of-two divider).
pub const CLK_DIVIDER_POWER_OF_TWO: u32 = 1 << 1;
/// Divider register value 0 means div-1 (default: 0 means div-N where N+1).
pub const CLK_DIVIDER_ONE_BASED: u32 = 1 << 0;
/// Divider table provided (use clk_div_table).
pub const CLK_DIVIDER_ALLOW_ZERO: u32 = 1 << 2;
/// Round-closest mode (round to nearest divider).
pub const CLK_DIVIDER_ROUND_CLOSEST: u32 = 1 << 4;
/// Use the read-only divider (cannot change rate).
pub const CLK_DIVIDER_READ_ONLY: u32 = 1 << 5;
/// Divider supports max-half (limited range).
pub const CLK_DIVIDER_MAX_AT_ZERO: u32 = 1 << 6;
/// Hiword mask register (write upper 16 bits = mask).
pub const CLK_DIVIDER_HIWORD_MASK: u32 = 1 << 3;
/// Big-endian register access.
pub const CLK_DIVIDER_BIG_ENDIAN: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// clk_mux flags
// ---------------------------------------------------------------------------

pub const CLK_MUX_INDEX_ONE: u32 = 1 << 0;
pub const CLK_MUX_INDEX_BIT: u32 = 1 << 1;
pub const CLK_MUX_HIWORD_MASK: u32 = 1 << 2;
pub const CLK_MUX_READ_ONLY: u32 = 1 << 3;
pub const CLK_MUX_ROUND_CLOSEST: u32 = 1 << 4;
pub const CLK_MUX_BIG_ENDIAN: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// clk_gate flags
// ---------------------------------------------------------------------------

/// Inverted enable bit: 1 = disabled.
pub const CLK_GATE_SET_TO_DISABLE: u32 = 1 << 0;
/// Hiword mask register (write upper 16 = mask).
pub const CLK_GATE_HIWORD_MASK: u32 = 1 << 1;
/// Big-endian register access.
pub const CLK_GATE_BIG_ENDIAN: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// clk_fractional_divider flags
// ---------------------------------------------------------------------------

pub const CLK_FRAC_DIVIDER_ZERO_BASED: u32 = 1 << 0;
pub const CLK_FRAC_DIVIDER_BIG_ENDIAN: u32 = 1 << 1;
pub const CLK_FRAC_DIVIDER_POWER_OF_TWO_PS: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// clk_div_table sentinel
// ---------------------------------------------------------------------------

/// End-of-table sentinel for `struct clk_div_table` arrays.
pub const CLK_DIV_TABLE_END_VAL: u32 = 0;
pub const CLK_DIV_TABLE_END_DIV: u32 = 0;

// ---------------------------------------------------------------------------
// Maximum number of parents a mux can select among
// ---------------------------------------------------------------------------

/// Practical upper bound on mux parents (kernel uses u8 index).
pub const CLK_MUX_MAX_PARENTS: usize = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_divider_flags_distinct_single_bit() {
        let f = [
            CLK_DIVIDER_ONE_BASED,
            CLK_DIVIDER_POWER_OF_TWO,
            CLK_DIVIDER_ALLOW_ZERO,
            CLK_DIVIDER_HIWORD_MASK,
            CLK_DIVIDER_ROUND_CLOSEST,
            CLK_DIVIDER_READ_ONLY,
            CLK_DIVIDER_MAX_AT_ZERO,
            CLK_DIVIDER_BIG_ENDIAN,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
    }

    #[test]
    fn test_mux_flags_dense_low_6_bits() {
        let f = [
            CLK_MUX_INDEX_ONE,
            CLK_MUX_INDEX_BIT,
            CLK_MUX_HIWORD_MASK,
            CLK_MUX_READ_ONLY,
            CLK_MUX_ROUND_CLOSEST,
            CLK_MUX_BIG_ENDIAN,
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v, 1 << i);
        }
        // OR of all = low 6 bits = 0x3F.
        let or_all = f.iter().fold(0u32, |a, &v| a | v);
        assert_eq!(or_all, 0x3F);
    }

    #[test]
    fn test_gate_flags_dense_low_3_bits() {
        let f = [CLK_GATE_SET_TO_DISABLE, CLK_GATE_HIWORD_MASK, CLK_GATE_BIG_ENDIAN];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v, 1 << i);
        }
    }

    #[test]
    fn test_fractional_flags_distinct() {
        let f = [
            CLK_FRAC_DIVIDER_ZERO_BASED,
            CLK_FRAC_DIVIDER_BIG_ENDIAN,
            CLK_FRAC_DIVIDER_POWER_OF_TWO_PS,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
    }

    #[test]
    fn test_div_table_sentinel_zero() {
        assert_eq!(CLK_DIV_TABLE_END_VAL, 0);
        assert_eq!(CLK_DIV_TABLE_END_DIV, 0);
    }

    #[test]
    fn test_max_mux_parents_fits_u8() {
        assert_eq!(CLK_MUX_MAX_PARENTS, u8::MAX as usize);
    }
}
