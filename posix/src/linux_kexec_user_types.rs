//! `<linux/kexec.h>` — `kexec_load(2)` / `kexec_file_load(2)` ABI.
//!
//! `kexec` lets the running kernel jump into a new kernel without a
//! firmware reset. `systemd-kexec`, `crash`, and the entire `kdump`
//! crash-collection pipeline rely on the constants below to set up
//! the new kernel's segments and to indicate special boot conditions.

// ---------------------------------------------------------------------------
// Syscall numbers on x86_64
// ---------------------------------------------------------------------------

pub const NR_KEXEC_LOAD: u32 = 246;
pub const NR_KEXEC_FILE_LOAD: u32 = 320;

// ---------------------------------------------------------------------------
// Architecture mask (low 16 bits of `flags`)
// ---------------------------------------------------------------------------

pub const KEXEC_ARCH_MASK: u32 = 0xFFFF_0000;
pub const KEXEC_ARCH_DEFAULT: u32 = 0;
pub const KEXEC_ARCH_386: u32 = 3 << 16;
pub const KEXEC_ARCH_X86_64: u32 = 62 << 16;
pub const KEXEC_ARCH_PPC: u32 = 20 << 16;
pub const KEXEC_ARCH_PPC64: u32 = 21 << 16;
pub const KEXEC_ARCH_ARM: u32 = 40 << 16;
pub const KEXEC_ARCH_AARCH64: u32 = 183 << 16;
pub const KEXEC_ARCH_RISCV: u32 = 243 << 16;

// ---------------------------------------------------------------------------
// `kexec_load` flags (top 16 bits free, low 16 used)
// ---------------------------------------------------------------------------

pub const KEXEC_ON_CRASH: u32 = 0x0000_0001;
pub const KEXEC_PRESERVE_CONTEXT: u32 = 0x0000_0002;
pub const KEXEC_UPDATE_ELFCOREHDR: u32 = 0x0000_0004;
pub const KEXEC_CRASH_HOTPLUG_SUPPORT: u32 = 0x0000_0008;

// ---------------------------------------------------------------------------
// `kexec_file_load` flags
// ---------------------------------------------------------------------------

pub const KEXEC_FILE_UNLOAD: u32 = 0x0000_0001;
pub const KEXEC_FILE_ON_CRASH: u32 = 0x0000_0002;
pub const KEXEC_FILE_NO_INITRAMFS: u32 = 0x0000_0004;
pub const KEXEC_FILE_DEBUG: u32 = 0x0000_0008;

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// Maximum segments in a kexec image.
pub const KEXEC_SEGMENT_MAX: usize = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syscall_numbers_x86_64() {
        // Verified against include/uapi/asm-generic/unistd.h x86_64 numbering.
        assert_eq!(NR_KEXEC_LOAD, 246);
        assert_eq!(NR_KEXEC_FILE_LOAD, 320);
    }

    #[test]
    fn test_arch_codes_in_high_half() {
        // All architecture codes occupy the upper 16 bits of `flags`.
        for c in [
            KEXEC_ARCH_386,
            KEXEC_ARCH_X86_64,
            KEXEC_ARCH_PPC,
            KEXEC_ARCH_PPC64,
            KEXEC_ARCH_ARM,
            KEXEC_ARCH_AARCH64,
            KEXEC_ARCH_RISCV,
        ] {
            assert_eq!(c & 0xFFFF, 0);
            assert_eq!(c & KEXEC_ARCH_MASK, c);
        }
        // x86_64 uses EM_X86_64 = 62 from <elf.h>.
        assert_eq!(KEXEC_ARCH_X86_64 >> 16, 62);
        // ARM uses EM_ARM = 40.
        assert_eq!(KEXEC_ARCH_ARM >> 16, 40);
        // AArch64 uses EM_AARCH64 = 183.
        assert_eq!(KEXEC_ARCH_AARCH64 >> 16, 183);
    }

    #[test]
    fn test_load_flags_pow2() {
        for &b in &[
            KEXEC_ON_CRASH,
            KEXEC_PRESERVE_CONTEXT,
            KEXEC_UPDATE_ELFCOREHDR,
            KEXEC_CRASH_HOTPLUG_SUPPORT,
        ] {
            assert!(b.is_power_of_two());
            // Flag bits stay clear of the architecture field.
            assert_eq!(b & KEXEC_ARCH_MASK, 0);
        }
    }

    #[test]
    fn test_file_load_flags_pow2() {
        for &b in &[
            KEXEC_FILE_UNLOAD,
            KEXEC_FILE_ON_CRASH,
            KEXEC_FILE_NO_INITRAMFS,
            KEXEC_FILE_DEBUG,
        ] {
            assert!(b.is_power_of_two());
        }
        // ON_CRASH bit position differs between the two syscalls — KEXEC_ON_CRASH
        // (load) is bit 0; KEXEC_FILE_ON_CRASH (file_load) is bit 1.
        assert_eq!(KEXEC_ON_CRASH, 1);
        assert_eq!(KEXEC_FILE_ON_CRASH, 2);
    }

    #[test]
    fn test_segment_max() {
        // KEXEC_SEGMENT_MAX has been 16 since the syscall was introduced.
        assert_eq!(KEXEC_SEGMENT_MAX, 16);
    }
}
