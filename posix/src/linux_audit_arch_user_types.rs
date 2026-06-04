//! `<linux/audit.h>` — `AUDIT_ARCH_*` machine-identifier tags.
//!
//! Each syscall record carries an `arch` field that identifies the
//! caller's instruction-set ABI. The encoding packs the ELF `e_machine`
//! number with a byte-order bit (LE/BE) and a 64-bit bit, so userspace
//! filters can match an absolute arch regardless of the host's
//! endianness.

// ---------------------------------------------------------------------------
// Encoding helpers
// ---------------------------------------------------------------------------

pub const __AUDIT_ARCH_64BIT: u32 = 0x8000_0000;
pub const __AUDIT_ARCH_LE: u32 = 0x4000_0000;
pub const __AUDIT_ARCH_CONVENTION_MASK: u32 = 0x3000_0000;
pub const __AUDIT_ARCH_CONVENTION_MIPS64_N32: u32 = 0x2000_0000;

// ---------------------------------------------------------------------------
// Selected ELF e_machine numbers
// ---------------------------------------------------------------------------

pub const EM_386: u32 = 3;
pub const EM_X86_64: u32 = 62;
pub const EM_ARM: u32 = 40;
pub const EM_AARCH64: u32 = 183;
pub const EM_PPC: u32 = 20;
pub const EM_PPC64: u32 = 21;
pub const EM_RISCV: u32 = 243;
pub const EM_S390: u32 = 22;
pub const EM_MIPS: u32 = 8;

// ---------------------------------------------------------------------------
// Composed `AUDIT_ARCH_*` tags
// ---------------------------------------------------------------------------

pub const AUDIT_ARCH_I386: u32 = EM_386 | __AUDIT_ARCH_LE;
pub const AUDIT_ARCH_X86_64: u32 = EM_X86_64 | __AUDIT_ARCH_64BIT | __AUDIT_ARCH_LE;
pub const AUDIT_ARCH_ARM: u32 = EM_ARM | __AUDIT_ARCH_LE;
pub const AUDIT_ARCH_AARCH64: u32 = EM_AARCH64 | __AUDIT_ARCH_64BIT | __AUDIT_ARCH_LE;
pub const AUDIT_ARCH_PPC: u32 = EM_PPC;
pub const AUDIT_ARCH_PPC64: u32 = EM_PPC64 | __AUDIT_ARCH_64BIT;
pub const AUDIT_ARCH_PPC64LE: u32 = EM_PPC64 | __AUDIT_ARCH_64BIT | __AUDIT_ARCH_LE;
pub const AUDIT_ARCH_RISCV32: u32 = EM_RISCV | __AUDIT_ARCH_LE;
pub const AUDIT_ARCH_RISCV64: u32 = EM_RISCV | __AUDIT_ARCH_64BIT | __AUDIT_ARCH_LE;
pub const AUDIT_ARCH_S390: u32 = EM_S390;
pub const AUDIT_ARCH_S390X: u32 = EM_S390 | __AUDIT_ARCH_64BIT;
pub const AUDIT_ARCH_MIPS: u32 = EM_MIPS;
pub const AUDIT_ARCH_MIPSEL: u32 = EM_MIPS | __AUDIT_ARCH_LE;
pub const AUDIT_ARCH_MIPS64: u32 = EM_MIPS | __AUDIT_ARCH_64BIT;
pub const AUDIT_ARCH_MIPS64N32: u32 =
    EM_MIPS | __AUDIT_ARCH_64BIT | __AUDIT_ARCH_CONVENTION_MIPS64_N32;
pub const AUDIT_ARCH_MIPSEL64: u32 = EM_MIPS | __AUDIT_ARCH_64BIT | __AUDIT_ARCH_LE;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_bits_distinct_and_single_bit_where_expected() {
        assert!(__AUDIT_ARCH_64BIT.is_power_of_two());
        assert!(__AUDIT_ARCH_LE.is_power_of_two());
        // 64BIT and LE occupy the two high bits.
        assert_eq!(__AUDIT_ARCH_64BIT | __AUDIT_ARCH_LE, 0xC000_0000);
        // Convention mask covers two bits below.
        assert_eq!(__AUDIT_ARCH_CONVENTION_MASK, 0x3000_0000);
        assert_eq!(
            __AUDIT_ARCH_CONVENTION_MIPS64_N32 & __AUDIT_ARCH_CONVENTION_MASK,
            __AUDIT_ARCH_CONVENTION_MIPS64_N32
        );
    }

    #[test]
    fn test_e_machine_values_match_elf() {
        // Spot-check against the ELF psABI registry.
        assert_eq!(EM_386, 3);
        assert_eq!(EM_X86_64, 62);
        assert_eq!(EM_ARM, 40);
        assert_eq!(EM_AARCH64, 183);
        assert_eq!(EM_PPC, 20);
        assert_eq!(EM_PPC64, 21);
        assert_eq!(EM_RISCV, 243);
        assert_eq!(EM_S390, 22);
        assert_eq!(EM_MIPS, 8);
    }

    #[test]
    fn test_x86_arch_tags() {
        // i386: 32-bit LE → only LE bit set.
        assert_eq!(AUDIT_ARCH_I386 & __AUDIT_ARCH_64BIT, 0);
        assert_eq!(AUDIT_ARCH_I386 & __AUDIT_ARCH_LE, __AUDIT_ARCH_LE);
        // x86_64: 64-bit LE.
        assert_eq!(AUDIT_ARCH_X86_64 & __AUDIT_ARCH_64BIT, __AUDIT_ARCH_64BIT);
        assert_eq!(AUDIT_ARCH_X86_64 & __AUDIT_ARCH_LE, __AUDIT_ARCH_LE);
        // Low 16 bits round-trip back to e_machine.
        assert_eq!(AUDIT_ARCH_I386 & 0xFFFF, EM_386);
        assert_eq!(AUDIT_ARCH_X86_64 & 0xFFFF, EM_X86_64);
    }

    #[test]
    fn test_arm_pair_distinct_widths() {
        // 32-bit ARM has no 64-bit bit; AArch64 has it.
        assert_eq!(AUDIT_ARCH_ARM & __AUDIT_ARCH_64BIT, 0);
        assert_eq!(
            AUDIT_ARCH_AARCH64 & __AUDIT_ARCH_64BIT,
            __AUDIT_ARCH_64BIT
        );
        assert_ne!(AUDIT_ARCH_ARM, AUDIT_ARCH_AARCH64);
    }

    #[test]
    fn test_ppc64_endianness_pair() {
        // PPC64 BE has no LE bit; PPC64LE has it.
        assert_eq!(AUDIT_ARCH_PPC64 & __AUDIT_ARCH_LE, 0);
        assert_eq!(AUDIT_ARCH_PPC64LE & __AUDIT_ARCH_LE, __AUDIT_ARCH_LE);
        // Both are 64-bit.
        assert_eq!(AUDIT_ARCH_PPC64 & __AUDIT_ARCH_64BIT, __AUDIT_ARCH_64BIT);
        assert_eq!(
            AUDIT_ARCH_PPC64LE & __AUDIT_ARCH_64BIT,
            __AUDIT_ARCH_64BIT
        );
    }

    #[test]
    fn test_mips_quartet_distinct() {
        let m = [
            AUDIT_ARCH_MIPS,
            AUDIT_ARCH_MIPSEL,
            AUDIT_ARCH_MIPS64,
            AUDIT_ARCH_MIPSEL64,
            AUDIT_ARCH_MIPS64N32,
        ];
        for (i, &a) in m.iter().enumerate() {
            for &b in &m[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // N32 carries the convention bits.
        assert_eq!(
            AUDIT_ARCH_MIPS64N32 & __AUDIT_ARCH_CONVENTION_MASK,
            __AUDIT_ARCH_CONVENTION_MIPS64_N32
        );
    }

    #[test]
    fn test_riscv_widths_share_machine_number() {
        assert_eq!(AUDIT_ARCH_RISCV32 & 0xFFFF, EM_RISCV);
        assert_eq!(AUDIT_ARCH_RISCV64 & 0xFFFF, EM_RISCV);
        // Both little-endian, but only RV64 has the 64-bit bit.
        assert_eq!(AUDIT_ARCH_RISCV32 & __AUDIT_ARCH_64BIT, 0);
        assert_eq!(
            AUDIT_ARCH_RISCV64 & __AUDIT_ARCH_64BIT,
            __AUDIT_ARCH_64BIT
        );
    }
}
