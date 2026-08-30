//! `<linux/nf_tables.h>` — Additional nftables constants.
//!
//! Supplementary nftables constants covering expression types,
//! verdict codes, and register definitions.

// ---------------------------------------------------------------------------
// Nftables verdict codes
// ---------------------------------------------------------------------------

/// Continue evaluation.
pub const NFT_CONTINUE: i32 = -1;
/// Terminate and accept.
pub const NFT_BREAK: i32 = -2;
/// Jump to chain.
pub const NFT_JUMP: i32 = -3;
/// Go to chain.
pub const NFT_GOTO: i32 = -4;
/// Return from chain.
pub const NFT_RETURN: i32 = -5;

// ---------------------------------------------------------------------------
// Nftables registers
// ---------------------------------------------------------------------------

/// Verdict register.
pub const NFT_REG_VERDICT: u32 = 0;
/// Register 1.
pub const NFT_REG_1: u32 = 1;
/// Register 2.
pub const NFT_REG_2: u32 = 2;
/// Register 3.
pub const NFT_REG_3: u32 = 3;
/// Register 4.
pub const NFT_REG_4: u32 = 4;
/// First 32-bit register.
pub const NFT_REG32_00: u32 = 8;
/// Second 32-bit register.
pub const NFT_REG32_01: u32 = 9;
/// Third 32-bit register.
pub const NFT_REG32_02: u32 = 10;
/// Fourth 32-bit register.
pub const NFT_REG32_03: u32 = 11;
/// Fifth 32-bit register.
pub const NFT_REG32_04: u32 = 12;
/// Sixth 32-bit register.
pub const NFT_REG32_05: u32 = 13;
/// Seventh 32-bit register.
pub const NFT_REG32_06: u32 = 14;
/// Eighth 32-bit register.
pub const NFT_REG32_07: u32 = 15;

// ---------------------------------------------------------------------------
// Nftables chain types
// ---------------------------------------------------------------------------

/// Filter chain.
pub const NFT_CHAIN_T_DEFAULT: u32 = 0;
/// Route chain.
pub const NFT_CHAIN_T_ROUTE: u32 = 1;
/// NAT chain.
pub const NFT_CHAIN_T_NAT: u32 = 2;

// ---------------------------------------------------------------------------
// Nftables set flags
// ---------------------------------------------------------------------------

/// Anonymous set.
pub const NFT_SET_ANONYMOUS: u32 = 1 << 0;
/// Constant set.
pub const NFT_SET_CONSTANT: u32 = 1 << 1;
/// Interval set.
pub const NFT_SET_INTERVAL: u32 = 1 << 2;
/// Map set.
pub const NFT_SET_MAP: u32 = 1 << 3;
/// Timeout set.
pub const NFT_SET_TIMEOUT: u32 = 1 << 4;
/// Eval set.
pub const NFT_SET_EVAL: u32 = 1 << 5;
/// Object set.
pub const NFT_SET_OBJECT: u32 = 1 << 6;
/// Concatenation set.
pub const NFT_SET_CONCAT: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verdict_codes_distinct() {
        let verdicts = [NFT_CONTINUE, NFT_BREAK, NFT_JUMP, NFT_GOTO, NFT_RETURN];
        for i in 0..verdicts.len() {
            for j in (i + 1)..verdicts.len() {
                assert_ne!(verdicts[i], verdicts[j]);
            }
        }
    }

    #[test]
    fn test_verdict_codes_negative() {
        assert!(NFT_CONTINUE < 0);
        assert!(NFT_BREAK < 0);
        assert!(NFT_JUMP < 0);
        assert!(NFT_GOTO < 0);
        assert!(NFT_RETURN < 0);
    }

    #[test]
    fn test_registers_distinct() {
        let regs = [
            NFT_REG_VERDICT,
            NFT_REG_1,
            NFT_REG_2,
            NFT_REG_3,
            NFT_REG_4,
            NFT_REG32_00,
            NFT_REG32_01,
            NFT_REG32_02,
            NFT_REG32_03,
            NFT_REG32_04,
            NFT_REG32_05,
            NFT_REG32_06,
            NFT_REG32_07,
        ];
        for i in 0..regs.len() {
            for j in (i + 1)..regs.len() {
                assert_ne!(regs[i], regs[j]);
            }
        }
    }

    #[test]
    fn test_chain_types_distinct() {
        let types = [NFT_CHAIN_T_DEFAULT, NFT_CHAIN_T_ROUTE, NFT_CHAIN_T_NAT];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_set_flags_no_overlap() {
        let flags = [
            NFT_SET_ANONYMOUS,
            NFT_SET_CONSTANT,
            NFT_SET_INTERVAL,
            NFT_SET_MAP,
            NFT_SET_TIMEOUT,
            NFT_SET_EVAL,
            NFT_SET_OBJECT,
            NFT_SET_CONCAT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_set_flags_power_of_two() {
        let flags = [
            NFT_SET_ANONYMOUS,
            NFT_SET_CONSTANT,
            NFT_SET_INTERVAL,
            NFT_SET_MAP,
            NFT_SET_TIMEOUT,
            NFT_SET_EVAL,
            NFT_SET_OBJECT,
            NFT_SET_CONCAT,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x} is not power of two", flag);
        }
    }
}
