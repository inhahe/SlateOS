//! `<linux/binfmts.h>` continuation — ELF auxiliary-vector setup limits
//! and `MAX_ARG_*` thresholds enforced by the kernel binfmt layer.
//!
//! Companion to `linux_binfmt_user_types`: that module covers the
//! `binfmt_misc` registration surface; this one covers the in-kernel
//! ELF/script loader limits and stack-setup constants.

// ---------------------------------------------------------------------------
// argv/envp / argument-page limits
// ---------------------------------------------------------------------------

/// Maximum bytes of argv+envp the kernel will set up for a new process.
///
/// Linux historically used `0x20000 * PAGE_SIZE` (= 128 MiB on
/// 4 KiB-page systems). Our equivalent uses 16 KiB pages, so the same
/// formula yields 512 MiB; expose the 0x20000 page count instead of a
/// byte count so the same constant works on every page size.
pub const MAX_ARG_PAGES: u32 = 32;

/// Hard cap on number of argv strings.
pub const MAX_ARG_STRINGS: u32 = 0x7FFF_FFFF;

/// Hard cap on individual argv string length (bytes incl. terminator).
pub const MAX_ARG_STRLEN: u32 = 32 * 4096;

// ---------------------------------------------------------------------------
// Interpreter recursion limits
// ---------------------------------------------------------------------------

/// `#!`-script interpreter chain depth limit (matches Linux 5.0+).
pub const BINPRM_BUF_SIZE: usize = 256;
pub const MAX_INTERP_RECURSION: u32 = 5;

// ---------------------------------------------------------------------------
// ELF e_type values (program-header level)
// ---------------------------------------------------------------------------

pub const ET_NONE: u16 = 0;
pub const ET_REL: u16 = 1;
pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;
pub const ET_CORE: u16 = 4;

// ---------------------------------------------------------------------------
// Loader output: setup_arg_pages flags
// ---------------------------------------------------------------------------

pub const STACK_TOP_TASK_SIZE: u32 = 0;
pub const EXSTACK_DEFAULT: u32 = 0;
pub const EXSTACK_DISABLE_X: u32 = 1;
pub const EXSTACK_ENABLE_X: u32 = 2;

// ---------------------------------------------------------------------------
// Stack-randomization tunable (bits of ASLR entropy for the stack)
// ---------------------------------------------------------------------------

/// Stack randomisation entropy bits on 64-bit hosts (Linux default).
pub const STACK_RND_MASK_64: u32 = 0x3F_FFFF; // 22 bits → 4 MiB span
/// Stack randomisation entropy bits on 32-bit hosts.
pub const STACK_RND_MASK_32: u32 = 0x7FF; // 11 bits → 8 KiB span

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arg_page_count_power_of_two() {
        assert_eq!(MAX_ARG_PAGES, 32);
        assert!(MAX_ARG_PAGES.is_power_of_two());
    }

    #[test]
    fn test_arg_string_caps() {
        // Max strings is one less than i32::MAX (signed-friendly).
        assert_eq!(MAX_ARG_STRINGS, i32::MAX as u32);
        // Max strlen is 128 KiB (32 pages of 4 KiB).
        assert_eq!(MAX_ARG_STRLEN, 32 * 4096);
        assert_eq!(MAX_ARG_STRLEN, 0x2_0000);
        assert!(MAX_ARG_STRLEN.is_power_of_two());
    }

    #[test]
    fn test_binprm_buf_and_recursion() {
        // 256-byte interpreter-line buffer (Linux 5.0+ raised it from 128).
        assert_eq!(BINPRM_BUF_SIZE, 256);
        assert!(BINPRM_BUF_SIZE.is_power_of_two());
        // Five-level interpreter recursion guard.
        assert_eq!(MAX_INTERP_RECURSION, 5);
    }

    #[test]
    fn test_elf_e_type_dense_0_to_4() {
        let t = [ET_NONE, ET_REL, ET_EXEC, ET_DYN, ET_CORE];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_exstack_codes_dense_with_default_zero() {
        assert_eq!(STACK_TOP_TASK_SIZE, 0);
        assert_eq!(EXSTACK_DEFAULT, 0);
        assert_eq!(EXSTACK_DISABLE_X, 1);
        assert_eq!(EXSTACK_ENABLE_X, 2);
        // DEFAULT and STACK_TOP_TASK_SIZE both intentionally use 0.
        assert_eq!(STACK_TOP_TASK_SIZE, EXSTACK_DEFAULT);
    }

    #[test]
    fn test_stack_rnd_masks_low_bit_runs() {
        // Both masks are contiguous low-bit runs.
        assert_eq!(STACK_RND_MASK_64.count_ones(), 22);
        assert_eq!(STACK_RND_MASK_32.count_ones(), 11);
        assert!((STACK_RND_MASK_64 + 1).is_power_of_two());
        assert!((STACK_RND_MASK_32 + 1).is_power_of_two());
        assert!(STACK_RND_MASK_64 > STACK_RND_MASK_32);
    }
}
