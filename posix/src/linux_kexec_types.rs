//! `<linux/kexec.h>` — kexec (kernel execution) constants.
//!
//! kexec loads a new kernel image into memory and reboots into it,
//! bypassing the BIOS/firmware boot sequence. Used for fast reboots,
//! crash dump kernels (kdump), and live kernel updates. kexec_load()
//! prepares the image; reboot(LINUX_REBOOT_CMD_KEXEC) triggers it.

// ---------------------------------------------------------------------------
// kexec_load flags
// ---------------------------------------------------------------------------

/// Load kernel for normal kexec reboot.
pub const KEXEC_ON_CRASH: u32 = 0x0000_0001;
/// Preserve memory for crash dump kernel.
pub const KEXEC_PRESERVE_CONTEXT: u32 = 0x0000_0002;
/// Update existing kexec segments (no full reload).
pub const KEXEC_UPDATE_ELFCOREHDR: u32 = 0x0000_0004;

// ---------------------------------------------------------------------------
// kexec_load segment types
// ---------------------------------------------------------------------------

/// Architecture mask in flags field.
pub const KEXEC_ARCH_MASK: u32 = 0xFFFF_0000;
/// Default (native) architecture.
pub const KEXEC_ARCH_DEFAULT: u32 = 0;
/// x86_64 architecture.
pub const KEXEC_ARCH_X86_64: u32 = 62 << 16;
/// i386 architecture.
pub const KEXEC_ARCH_386: u32 = 3 << 16;
/// ARM64 (AArch64) architecture.
pub const KEXEC_ARCH_AARCH64: u32 = 183 << 16;
/// RISC-V architecture.
pub const KEXEC_ARCH_RISCV: u32 = 243 << 16;

// ---------------------------------------------------------------------------
// kexec_file_load flags
// ---------------------------------------------------------------------------

/// Don't execute the kernel, just verify the signature.
pub const KEXEC_FILE_NO_INITRAMFS: u32 = 0x0000_0004;
/// Unload the currently loaded kexec kernel.
pub const KEXEC_FILE_UNLOAD: u32 = 0x0000_0001;
/// Load image for crash kernel slot.
pub const KEXEC_FILE_ON_CRASH: u32 = 0x0000_0002;

// ---------------------------------------------------------------------------
// Segment limits
// ---------------------------------------------------------------------------

/// Maximum number of kexec segments.
pub const KEXEC_SEGMENT_MAX: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_flags_no_overlap() {
        let flags = [
            KEXEC_ON_CRASH,
            KEXEC_PRESERVE_CONTEXT,
            KEXEC_UPDATE_ELFCOREHDR,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_arch_values_distinct() {
        let archs = [
            KEXEC_ARCH_DEFAULT,
            KEXEC_ARCH_X86_64,
            KEXEC_ARCH_386,
            KEXEC_ARCH_AARCH64,
            KEXEC_ARCH_RISCV,
        ];
        for i in 0..archs.len() {
            for j in (i + 1)..archs.len() {
                assert_ne!(archs[i], archs[j]);
            }
        }
    }

    #[test]
    fn test_arch_mask_extracts_arch() {
        assert_eq!(KEXEC_ARCH_X86_64 & KEXEC_ARCH_MASK, KEXEC_ARCH_X86_64);
        assert_eq!(KEXEC_ARCH_386 & KEXEC_ARCH_MASK, KEXEC_ARCH_386);
        assert_eq!(KEXEC_ARCH_DEFAULT & KEXEC_ARCH_MASK, 0);
    }

    #[test]
    fn test_file_load_flags_distinct() {
        let flags = [
            KEXEC_FILE_UNLOAD,
            KEXEC_FILE_ON_CRASH,
            KEXEC_FILE_NO_INITRAMFS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_segment_max() {
        assert_eq!(KEXEC_SEGMENT_MAX, 16);
    }
}
