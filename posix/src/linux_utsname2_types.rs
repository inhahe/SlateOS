//! `<linux/utsname.h>` — Additional UTS name constants.
//!
//! Supplementary UTS name constants covering field lengths,
//! domain name limits, and version string formats.

// ---------------------------------------------------------------------------
// UTS field lengths
// ---------------------------------------------------------------------------

/// System name length (e.g., "Linux").
pub const UTS_SYSNAME_LEN: u32 = 65;
/// Node name length (hostname).
pub const UTS_NODENAME_LEN: u32 = 65;
/// Release string length (e.g., "6.1.0").
pub const UTS_RELEASE_LEN: u32 = 65;
/// Version string length.
pub const UTS_VERSION_LEN: u32 = 65;
/// Machine name length (e.g., "x86_64").
pub const UTS_MACHINE_LEN: u32 = 65;
/// Domain name length.
pub const UTS_DOMAINNAME_LEN: u32 = 65;

// ---------------------------------------------------------------------------
// New UTS namespace lengths (for uname syscall)
// ---------------------------------------------------------------------------

/// Total fields in utsname.
pub const UTS_FIELD_COUNT: u32 = 6;

// ---------------------------------------------------------------------------
// Machine type constants
// ---------------------------------------------------------------------------

/// x86_64 machine.
pub const MACH_X86_64: u32 = 0;
/// aarch64 machine.
pub const MACH_AARCH64: u32 = 1;
/// riscv64 machine.
pub const MACH_RISCV64: u32 = 2;
/// s390x machine.
pub const MACH_S390X: u32 = 3;
/// ppc64le machine.
pub const MACH_PPC64LE: u32 = 4;
/// loongarch64 machine.
pub const MACH_LOONGARCH64: u32 = 5;

// ---------------------------------------------------------------------------
// Kernel version encoding
// ---------------------------------------------------------------------------

/// Encode version number: (major << 16) | (minor << 8) | patch.
pub const KERNEL_VERSION_SHIFT_MAJOR: u32 = 16;
/// Minor version shift.
pub const KERNEL_VERSION_SHIFT_MINOR: u32 = 8;
/// Version mask.
pub const KERNEL_VERSION_MASK: u32 = 0xFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_lengths_equal() {
        // All UTS fields have the same length
        assert_eq!(UTS_SYSNAME_LEN, UTS_NODENAME_LEN);
        assert_eq!(UTS_NODENAME_LEN, UTS_RELEASE_LEN);
        assert_eq!(UTS_RELEASE_LEN, UTS_VERSION_LEN);
        assert_eq!(UTS_VERSION_LEN, UTS_MACHINE_LEN);
        assert_eq!(UTS_MACHINE_LEN, UTS_DOMAINNAME_LEN);
    }

    #[test]
    fn test_field_length_value() {
        assert_eq!(UTS_SYSNAME_LEN, 65);
    }

    #[test]
    fn test_field_count() {
        assert_eq!(UTS_FIELD_COUNT, 6);
    }

    #[test]
    fn test_machine_types_distinct() {
        let types = [
            MACH_X86_64,
            MACH_AARCH64,
            MACH_RISCV64,
            MACH_S390X,
            MACH_PPC64LE,
            MACH_LOONGARCH64,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_version_encoding() {
        // Linux 6.1.0 = (6 << 16) | (1 << 8) | 0 = 0x060100
        let version = (6 << KERNEL_VERSION_SHIFT_MAJOR) | (1 << KERNEL_VERSION_SHIFT_MINOR) | 0;
        assert_eq!(version, 0x060100);
    }
}
