//! `<linux/kexec.h>` — Kexec/kdump constants.
//!
//! kexec allows booting a new kernel from a running kernel without
//! going through BIOS/UEFI. kdump uses kexec to boot a crash kernel
//! that captures a memory dump after a panic. This module defines
//! the syscall flags, segment types, and architecture constants.

// ---------------------------------------------------------------------------
// Kexec flags (for kexec_load / kexec_file_load)
// ---------------------------------------------------------------------------

/// Load into crash kernel region.
pub const KEXEC_ON_CRASH: u32 = 0x0000_0001;
/// Preserve context (hibernation-style).
pub const KEXEC_PRESERVE_CONTEXT: u32 = 0x0000_0002;
/// Update ELF headers.
pub const KEXEC_UPDATE_ELFCOREHDR: u32 = 0x0000_0004;

// ---------------------------------------------------------------------------
// Kexec file flags (kexec_file_load specific)
// ---------------------------------------------------------------------------

/// Unload the crash kernel.
pub const KEXEC_FILE_UNLOAD: u32 = 0x0000_0001;
/// Load into crash kernel region (file variant).
pub const KEXEC_FILE_ON_CRASH: u32 = 0x0000_0002;
/// No initramfs.
pub const KEXEC_FILE_NO_INITRAMFS: u32 = 0x0000_0004;
/// Debug output.
pub const KEXEC_FILE_DEBUG: u32 = 0x0000_0008;

// ---------------------------------------------------------------------------
// Architecture types
// ---------------------------------------------------------------------------

/// x86_64.
pub const KEXEC_ARCH_X86_64: u32 = 62 << 16;
/// i386.
pub const KEXEC_ARCH_386: u32 = 3 << 16;
/// ARM.
pub const KEXEC_ARCH_ARM: u32 = 40 << 16;
/// AArch64.
pub const KEXEC_ARCH_AARCH64: u32 = 183 << 16;
/// RISC-V.
pub const KEXEC_ARCH_RISCV: u32 = 243 << 16;
/// PowerPC64.
pub const KEXEC_ARCH_PPC64: u32 = 21 << 16;
/// S390X.
pub const KEXEC_ARCH_S390: u32 = 22 << 16;
/// Default: use running kernel's arch.
pub const KEXEC_ARCH_DEFAULT: u32 = 0;
/// Mask to extract arch from flags.
pub const KEXEC_ARCH_MASK: u32 = 0xFFFF_0000;

// ---------------------------------------------------------------------------
// Segment types
// ---------------------------------------------------------------------------

/// Maximum number of segments.
pub const KEXEC_SEGMENT_MAX: usize = 16;

// ---------------------------------------------------------------------------
// Crash kernel memory
// ---------------------------------------------------------------------------

/// Crash kernel reserved (magic for /proc/iomem).
pub const CRASH_KERNEL_RESERVED: u32 = 0x4352_4153; // "CRAS" in ASCII

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kexec_flags_distinct() {
        let flags = [
            KEXEC_ON_CRASH,
            KEXEC_PRESERVE_CONTEXT,
            KEXEC_UPDATE_ELFCOREHDR,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_kexec_flags_powers_of_two() {
        let flags = [
            KEXEC_ON_CRASH,
            KEXEC_PRESERVE_CONTEXT,
            KEXEC_UPDATE_ELFCOREHDR,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_file_flags_powers_of_two() {
        let flags = [
            KEXEC_FILE_UNLOAD,
            KEXEC_FILE_ON_CRASH,
            KEXEC_FILE_NO_INITRAMFS,
            KEXEC_FILE_DEBUG,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_arch_types_distinct() {
        let arches = [
            KEXEC_ARCH_X86_64,
            KEXEC_ARCH_386,
            KEXEC_ARCH_ARM,
            KEXEC_ARCH_AARCH64,
            KEXEC_ARCH_RISCV,
            KEXEC_ARCH_PPC64,
            KEXEC_ARCH_S390,
        ];
        for i in 0..arches.len() {
            for j in (i + 1)..arches.len() {
                assert_ne!(arches[i], arches[j]);
            }
        }
    }

    #[test]
    fn test_arch_x86_64() {
        assert_eq!(KEXEC_ARCH_X86_64, 62 << 16);
    }

    #[test]
    fn test_arch_mask() {
        assert_eq!(KEXEC_ARCH_MASK, 0xFFFF_0000);
        // All arches should be fully within the mask.
        assert_eq!(KEXEC_ARCH_X86_64 & !KEXEC_ARCH_MASK, 0);
        assert_eq!(KEXEC_ARCH_386 & !KEXEC_ARCH_MASK, 0);
    }

    #[test]
    fn test_segment_max() {
        assert_eq!(KEXEC_SEGMENT_MAX, 16);
    }
}
