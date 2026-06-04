//! `<sched.h>` — clone() exit-signal mask and architecture entry points.
//!
//! The low byte of clone()'s flags argument is the exit signal — the
//! signal the kernel sends the parent when the child exits. The high
//! bytes are CLONE_* flags. This module covers the masks and helper
//! constants used by libc wrappers.

// ---------------------------------------------------------------------------
// Exit-signal mask (low byte of clone flags)
// ---------------------------------------------------------------------------

/// Low 8 bits of clone(2) flags are the exit signal number.
pub const CSIGNAL_MASK: u64 = 0x0000_0000_0000_00FF;

/// Signal sent to parent when the child terminates (SIGCHLD = 17 on x86_64).
pub const CSIGNAL_SIGCHLD: u64 = 17;

// ---------------------------------------------------------------------------
// clone() return value semantics
// ---------------------------------------------------------------------------

/// Return value in the child process.
pub const CLONE_RETURN_CHILD: i32 = 0;

/// Error return: kernel returns negative errno (signed long).
pub const CLONE_RETURN_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// Standard child-stack sizes
// ---------------------------------------------------------------------------

/// Glibc default child thread stack (8 MiB).
pub const CLONE_DEFAULT_STACK_SIZE: usize = 8 * 1024 * 1024;

/// Minimum sensible child stack — one page (4 KiB).
pub const CLONE_MIN_STACK_SIZE: usize = 4096;

// ---------------------------------------------------------------------------
// Architecture-specific clone() syscall numbers
// ---------------------------------------------------------------------------

pub const NR_CLONE_X86_64: u32 = 56;
pub const NR_CLONE_AARCH64: u32 = 220;
pub const NR_CLONE_I386: u32 = 120;
pub const NR_CLONE_POWERPC: u32 = 120;
pub const NR_CLONE_RISCV: u32 = 220;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csignal_mask_is_low_byte() {
        assert_eq!(CSIGNAL_MASK, 0xFF);
        // Extract: (flags & CSIGNAL_MASK) yields the exit signal number.
        assert_eq!(CSIGNAL_MASK as u8, 0xFF);
        // High bits are zero.
        assert_eq!(CSIGNAL_MASK >> 8, 0);
    }

    #[test]
    fn test_sigchld_within_csignal_mask() {
        assert_eq!(CSIGNAL_SIGCHLD, 17);
        assert_eq!(CSIGNAL_SIGCHLD & CSIGNAL_MASK, CSIGNAL_SIGCHLD);
    }

    #[test]
    fn test_return_values_distinct() {
        assert_eq!(CLONE_RETURN_CHILD, 0);
        assert_eq!(CLONE_RETURN_ERROR, -1);
        assert_ne!(CLONE_RETURN_CHILD, CLONE_RETURN_ERROR);
    }

    #[test]
    fn test_stack_sizes_consistent() {
        // 8 MiB default.
        assert_eq!(CLONE_DEFAULT_STACK_SIZE, 8 * 1024 * 1024);
        assert_eq!(CLONE_DEFAULT_STACK_SIZE / (1024 * 1024), 8);
        // 4 KiB minimum.
        assert_eq!(CLONE_MIN_STACK_SIZE, 4096);
        assert!(CLONE_MIN_STACK_SIZE.is_power_of_two());
        assert!(CLONE_DEFAULT_STACK_SIZE > CLONE_MIN_STACK_SIZE);
    }

    #[test]
    fn test_syscall_numbers_per_arch() {
        // x86_64 and i386 have very different ABIs.
        assert_eq!(NR_CLONE_X86_64, 56);
        assert_eq!(NR_CLONE_I386, 120);
        // AArch64 shares 220 with RISC-V (both use generic syscall table).
        assert_eq!(NR_CLONE_AARCH64, 220);
        assert_eq!(NR_CLONE_RISCV, 220);
        assert_eq!(NR_CLONE_AARCH64, NR_CLONE_RISCV);
        // PowerPC matches i386.
        assert_eq!(NR_CLONE_POWERPC, NR_CLONE_I386);
    }
}
