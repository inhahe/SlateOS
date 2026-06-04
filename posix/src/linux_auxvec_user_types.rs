//! `<linux/auxvec.h>` — sizing / vector-bound constants.
//!
//! Complement to `linux_auxv_user_types`: the *tags* live there, the
//! *vector sizing* lives here. The kernel exports `AT_VECTOR_SIZE_*`
//! macros used by the ELF loader to pre-size the auxv array before
//! filling it in.

// ---------------------------------------------------------------------------
// Per-arch base count and slack
// ---------------------------------------------------------------------------

/// Number of architecture-independent entries the loader always writes.
///
/// Counts `AT_HWCAP`, `AT_PAGESZ`, `AT_CLKTCK`, `AT_PHDR`, `AT_PHENT`,
/// `AT_PHNUM`, `AT_BASE`, `AT_FLAGS`, `AT_ENTRY`, `AT_UID`, `AT_EUID`,
/// `AT_GID`, `AT_EGID`, `AT_SECURE`, `AT_RANDOM`, `AT_HWCAP2`,
/// `AT_EXECFN`, `AT_PLATFORM`, plus the terminating `AT_NULL`.
pub const AT_VECTOR_SIZE_BASE: usize = 20;

/// Per-arch override budget — architectures with their own AT tags
/// (e.g. x86 `AT_SYSINFO_EHDR`, MIPS `AT_BASE_PLATFORM`) reserve a few
/// more slots above the base.
pub const AT_VECTOR_SIZE_ARCH: usize = 3;

/// Total vector slot count visible to userspace.
pub const AT_VECTOR_SIZE: usize = 2 * (AT_VECTOR_SIZE_ARCH + AT_VECTOR_SIZE_BASE + 1);

// ---------------------------------------------------------------------------
// VDSO / vsyscall placement (informational — varies per arch)
// ---------------------------------------------------------------------------

/// On x86_64, the VDSO is mapped into a single page-aligned region; the
/// loader stores its base in `AT_SYSINFO_EHDR`.
pub const VDSO_ALIGN: usize = 4096;

/// Loader rounds the auxv pointer up to a 16-byte boundary before
/// stashing it after `envp`'s null terminator.
pub const AUXV_PTR_ALIGN: usize = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_size_covers_arch_independent_tags() {
        // 18 mandatory AT_* entries + AT_NULL terminator + 1 slack slot
        // == 20. If the loader's mandatory set ever changes, this must
        // change in lockstep with the kernel.
        assert_eq!(AT_VECTOR_SIZE_BASE, 20);
    }

    #[test]
    fn test_arch_slack_is_three() {
        assert_eq!(AT_VECTOR_SIZE_ARCH, 3);
        // ARCH slack is small — arches pick the few tags they care
        // about (e.g. x86 takes AT_SYSINFO_EHDR + AT_SYSINFO).
        assert!(AT_VECTOR_SIZE_ARCH < AT_VECTOR_SIZE_BASE);
    }

    #[test]
    fn test_total_size_doubles_arch_plus_base_plus_one() {
        // Total = 2 * (BASE + ARCH + 1) because each entry is a
        // (key, value) pair and the kernel macro counts u64 slots.
        assert_eq!(AT_VECTOR_SIZE, 2 * (AT_VECTOR_SIZE_BASE + AT_VECTOR_SIZE_ARCH + 1));
        // Even number of slots (key/value pairs always come in twos).
        assert_eq!(AT_VECTOR_SIZE % 2, 0);
    }

    #[test]
    fn test_alignment_constants_power_of_two() {
        assert!(VDSO_ALIGN.is_power_of_two());
        assert!(AUXV_PTR_ALIGN.is_power_of_two());
        // VDSO is page-sized (4 KiB on x86, but the constant is
        // generic-page conservatism).
        assert_eq!(VDSO_ALIGN, 4096);
        // 16-byte (xmm) alignment matches the SysV x86_64 stack ABI.
        assert_eq!(AUXV_PTR_ALIGN, 16);
        assert!(VDSO_ALIGN > AUXV_PTR_ALIGN);
    }
}
