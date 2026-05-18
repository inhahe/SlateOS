//! `<linux/audit.h>` — Audit architecture and machine type constants.
//!
//! The Linux audit subsystem uses architecture identifiers to tag
//! syscall audit records, allowing userspace tools to correctly
//! decode syscall numbers (which differ between architectures).
//! The AUDIT_ARCH_* values encode both the ELF machine type and
//! the convention bit (64-bit, little/big endian).

// ---------------------------------------------------------------------------
// Audit architecture convention bits
// ---------------------------------------------------------------------------

/// 64-bit architecture flag.
pub const AUDIT_ARCH_64BIT: u32 = 0x8000_0000;
/// Little-endian architecture flag.
pub const AUDIT_ARCH_LE: u32 = 0x4000_0000;
/// Convention mask (64-bit + endianness).
pub const AUDIT_ARCH_CONVENTION_MASK: u32 = 0xC000_0000;

// ---------------------------------------------------------------------------
// Audit architecture IDs (common platforms)
// ---------------------------------------------------------------------------

/// x86 (i386, 32-bit, little-endian).
pub const AUDIT_ARCH_I386: u32 = 3 | AUDIT_ARCH_LE;
/// x86_64 (amd64, 64-bit, little-endian).
pub const AUDIT_ARCH_X86_64: u32 = 62 | AUDIT_ARCH_64BIT | AUDIT_ARCH_LE;
/// ARM (32-bit, little-endian).
pub const AUDIT_ARCH_ARM: u32 = 40 | AUDIT_ARCH_LE;
/// AArch64 (ARM 64-bit, little-endian).
pub const AUDIT_ARCH_AARCH64: u32 = 183 | AUDIT_ARCH_64BIT | AUDIT_ARCH_LE;
/// MIPS (32-bit, big-endian).
pub const AUDIT_ARCH_MIPS: u32 = 8;
/// MIPS64 (64-bit, big-endian).
pub const AUDIT_ARCH_MIPS64: u32 = 8 | AUDIT_ARCH_64BIT;
/// PowerPC (32-bit, big-endian).
pub const AUDIT_ARCH_PPC: u32 = 20;
/// PowerPC 64 (64-bit, big-endian).
pub const AUDIT_ARCH_PPC64: u32 = 21 | AUDIT_ARCH_64BIT;
/// PowerPC 64 LE.
pub const AUDIT_ARCH_PPC64LE: u32 = 21 | AUDIT_ARCH_64BIT | AUDIT_ARCH_LE;
/// RISC-V 64 (little-endian).
pub const AUDIT_ARCH_RISCV64: u32 = 243 | AUDIT_ARCH_64BIT | AUDIT_ARCH_LE;
/// s390x (64-bit, big-endian).
pub const AUDIT_ARCH_S390X: u32 = 22 | AUDIT_ARCH_64BIT;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convention_bits_no_overlap() {
        assert!(AUDIT_ARCH_64BIT.is_power_of_two());
        assert!(AUDIT_ARCH_LE.is_power_of_two());
        assert_eq!(AUDIT_ARCH_64BIT & AUDIT_ARCH_LE, 0);
    }

    #[test]
    fn test_x86_64_has_64bit_and_le() {
        assert_ne!(AUDIT_ARCH_X86_64 & AUDIT_ARCH_64BIT, 0);
        assert_ne!(AUDIT_ARCH_X86_64 & AUDIT_ARCH_LE, 0);
    }

    #[test]
    fn test_i386_is_32bit_le() {
        assert_eq!(AUDIT_ARCH_I386 & AUDIT_ARCH_64BIT, 0);
        assert_ne!(AUDIT_ARCH_I386 & AUDIT_ARCH_LE, 0);
    }

    #[test]
    fn test_mips_is_big_endian() {
        assert_eq!(AUDIT_ARCH_MIPS & AUDIT_ARCH_LE, 0);
    }

    #[test]
    fn test_architectures_distinct() {
        let archs = [
            AUDIT_ARCH_I386, AUDIT_ARCH_X86_64, AUDIT_ARCH_ARM,
            AUDIT_ARCH_AARCH64, AUDIT_ARCH_MIPS, AUDIT_ARCH_MIPS64,
            AUDIT_ARCH_PPC, AUDIT_ARCH_PPC64, AUDIT_ARCH_PPC64LE,
            AUDIT_ARCH_RISCV64, AUDIT_ARCH_S390X,
        ];
        for i in 0..archs.len() {
            for j in (i + 1)..archs.len() {
                assert_ne!(archs[i], archs[j]);
            }
        }
    }
}
