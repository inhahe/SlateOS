//! `<linux/kexec.h>` — Additional kexec constants.
//!
//! Supplementary kexec constants covering segment types,
//! file load flags, and architecture flags.

// ---------------------------------------------------------------------------
// kexec flags
// ---------------------------------------------------------------------------

/// On crash — kexec on panic.
pub const KEXEC_ON_CRASH: u32 = 0x00000001;
/// Preserve context — keep memory state.
pub const KEXEC_PRESERVE_CONTEXT: u32 = 0x00000002;
/// Update elfcorehdr.
pub const KEXEC_UPDATE_ELFCOREHDR: u32 = 0x00000004;

// ---------------------------------------------------------------------------
// kexec file load flags
// ---------------------------------------------------------------------------

/// Unload the current kernel image.
pub const KEXEC_FILE_UNLOAD: u32 = 0x00000001;
/// Load on crash.
pub const KEXEC_FILE_ON_CRASH: u32 = 0x00000002;
/// No initramfs.
pub const KEXEC_FILE_NO_INITRAMFS: u32 = 0x00000004;
/// Debug.
pub const KEXEC_FILE_DEBUG: u32 = 0x00000008;

// ---------------------------------------------------------------------------
// kexec architecture flags
// ---------------------------------------------------------------------------

/// x86_64 architecture.
pub const KEXEC_ARCH_X86_64: u32 = 62 << 16;
/// i386 architecture.
pub const KEXEC_ARCH_386: u32 = 3 << 16;
/// ARM architecture.
pub const KEXEC_ARCH_ARM: u32 = 40 << 16;
/// AARCH64 architecture.
pub const KEXEC_ARCH_AARCH64: u32 = 183 << 16;
/// RISC-V architecture.
pub const KEXEC_ARCH_RISCV: u32 = 243 << 16;
/// Default (native) architecture.
pub const KEXEC_ARCH_DEFAULT: u32 = 0;

// ---------------------------------------------------------------------------
// kexec segment flags
// ---------------------------------------------------------------------------

/// Maximum segments.
pub const KEXEC_SEGMENT_MAX: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            KEXEC_ON_CRASH,
            KEXEC_PRESERVE_CONTEXT,
            KEXEC_UPDATE_ELFCOREHDR,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_file_flags_no_overlap() {
        let flags = [
            KEXEC_FILE_UNLOAD,
            KEXEC_FILE_ON_CRASH,
            KEXEC_FILE_NO_INITRAMFS,
            KEXEC_FILE_DEBUG,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_arch_flags_distinct() {
        let archs = [
            KEXEC_ARCH_X86_64,
            KEXEC_ARCH_386,
            KEXEC_ARCH_ARM,
            KEXEC_ARCH_AARCH64,
            KEXEC_ARCH_RISCV,
            KEXEC_ARCH_DEFAULT,
        ];
        for i in 0..archs.len() {
            for j in (i + 1)..archs.len() {
                assert_ne!(archs[i], archs[j]);
            }
        }
    }

    #[test]
    fn test_segment_max() {
        assert_eq!(KEXEC_SEGMENT_MAX, 16);
    }
}
