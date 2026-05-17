//! `<linux/sched/task_stack.h>` — Process stack constants.
//!
//! Each kernel thread and user process has associated stacks. The
//! kernel stack is small and fixed-size (used during syscalls and
//! interrupts). The user stack grows dynamically (via page faults
//! on the guard page). Stack overflow protection uses guard pages
//! and, on x86_64, shadow stacks (CET) for return-address integrity.

// ---------------------------------------------------------------------------
// Kernel stack sizes
// ---------------------------------------------------------------------------

/// Kernel stack size on x86_64 (16 KiB = 4 pages at 4K).
pub const THREAD_SIZE_X86_64: u32 = 16384;
/// Kernel stack size on ARM64 (16 KiB).
pub const THREAD_SIZE_ARM64: u32 = 16384;
/// Interrupt stack size (separate stack for IRQ handling).
pub const IRQ_STACK_SIZE: u32 = 16384;

// ---------------------------------------------------------------------------
// User stack defaults
// ---------------------------------------------------------------------------

/// Default user stack size (8 MiB, RLIMIT_STACK default).
pub const USER_STACK_DEFAULT: u32 = 8 * 1024 * 1024;
/// Minimum user stack size.
pub const USER_STACK_MIN: u32 = 128 * 1024;
/// Maximum user stack size (typically unlimited or 256 MiB).
pub const USER_STACK_MAX: u32 = 256 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Stack guard page constants
// ---------------------------------------------------------------------------

/// Number of guard pages below the stack (gap to detect overflow).
pub const STACK_GUARD_GAP_PAGES: u32 = 256;
/// Size of the stack guard gap in bytes (1 MiB at 4K pages).
pub const STACK_GUARD_GAP_BYTES: u32 = 256 * 4096;

// ---------------------------------------------------------------------------
// Stack growth direction
// ---------------------------------------------------------------------------

/// Stack grows downward (x86, ARM: push decrements SP).
pub const STACK_GROWS_DOWN: u32 = 0;
/// Stack grows upward (PA-RISC, some MIPS: push increments SP).
pub const STACK_GROWS_UP: u32 = 1;

// ---------------------------------------------------------------------------
// Shadow stack (CET) constants (x86_64)
// ---------------------------------------------------------------------------

/// Shadow stack is enabled.
pub const SHADOW_STACK_ENABLED: u32 = 0x01;
/// Shadow stack write-protect (cannot be written by regular stores).
pub const SHADOW_STACK_WRITE_PROTECT: u32 = 0x02;
/// Shadow stack push token validation.
pub const SHADOW_STACK_PUSH_TOKEN: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_stack_sizes() {
        assert!(THREAD_SIZE_X86_64.is_power_of_two());
        assert!(THREAD_SIZE_ARM64.is_power_of_two());
        assert!(IRQ_STACK_SIZE.is_power_of_two());
    }

    #[test]
    fn test_user_stack_range() {
        assert!(USER_STACK_MIN < USER_STACK_DEFAULT);
        assert!(USER_STACK_DEFAULT < USER_STACK_MAX);
    }

    #[test]
    fn test_guard_gap() {
        assert!(STACK_GUARD_GAP_PAGES > 0);
        assert_eq!(STACK_GUARD_GAP_BYTES, STACK_GUARD_GAP_PAGES * 4096);
    }

    #[test]
    fn test_growth_directions_distinct() {
        assert_ne!(STACK_GROWS_DOWN, STACK_GROWS_UP);
    }

    #[test]
    fn test_shadow_stack_flags_no_overlap() {
        let flags = [
            SHADOW_STACK_ENABLED, SHADOW_STACK_WRITE_PROTECT,
            SHADOW_STACK_PUSH_TOKEN,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
