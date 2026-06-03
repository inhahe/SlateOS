//! `pkey_*` — Memory Protection Keys ABI.
//!
//! MPK (Intel "PKU", POWER's "AMR") lets userspace tag pages with one
//! of 16 protection-key indexes and toggle read/write access via the
//! PKRU register without an `mprotect` syscall. Glibc uses it for
//! `pthread_setspecific`-style fast TLS slots and stack canaries;
//! sandboxes use it for in-process isolation.

// ---------------------------------------------------------------------------
// `pkey_alloc` flag and access-rights bits
// ---------------------------------------------------------------------------

/// `pkey_alloc(flags, ...)` flags — currently the kernel rejects every
/// bit. Defined as 0 so call sites can be explicit.
pub const PKEY_ALLOC_FLAGS_RESERVED: u32 = 0;

pub const PKEY_DISABLE_ACCESS: u32 = 1 << 0;
pub const PKEY_DISABLE_WRITE: u32 = 1 << 1;
pub const PKEY_DISABLE_EXECUTE: u32 = 1 << 2;

/// Mask of every documented access-rights bit.
pub const PKEY_ACCESS_MASK: u32 =
    PKEY_DISABLE_ACCESS | PKEY_DISABLE_WRITE | PKEY_DISABLE_EXECUTE;

// ---------------------------------------------------------------------------
// Number of protection keys exposed by the architecture
// ---------------------------------------------------------------------------

/// On x86 with PKU, exactly 16 keys are available (keys 0–15). Key 0
/// is reserved as the default for new mappings.
pub const ARCH_PKEY_COUNT_X86: u32 = 16;
/// Powers also report 32 on some chips — the upper bound the syscall
/// will hand out.
pub const ARCH_PKEY_COUNT_MAX: u32 = 32;
/// Default key implicitly assigned to anonymous mappings.
pub const PKEY_DEFAULT: i32 = 0;

// ---------------------------------------------------------------------------
// Syscalls (x86_64)
// ---------------------------------------------------------------------------

pub const NR_PKEY_MPROTECT: u32 = 329;
pub const NR_PKEY_ALLOC: u32 = 330;
pub const NR_PKEY_FREE: u32 = 331;

// ---------------------------------------------------------------------------
// `prot` argument shared with `mprotect`
// ---------------------------------------------------------------------------

pub const PROT_NONE: u32 = 0x0;
pub const PROT_READ: u32 = 0x1;
pub const PROT_WRITE: u32 = 0x2;
pub const PROT_EXEC: u32 = 0x4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disable_bits_single_bit_low_3() {
        let d = [PKEY_DISABLE_ACCESS, PKEY_DISABLE_WRITE, PKEY_DISABLE_EXECUTE];
        let mut or = 0u32;
        for (i, &v) in d.iter().enumerate() {
            assert!(v.is_power_of_two());
            assert_eq!(v, 1 << i);
            or |= v;
        }
        assert_eq!(or, 0x7);
        assert_eq!(PKEY_ACCESS_MASK, 0x7);
    }

    #[test]
    fn test_alloc_flags_currently_zero() {
        // pkey_alloc rejects any non-zero flag bit today.
        assert_eq!(PKEY_ALLOC_FLAGS_RESERVED, 0);
    }

    #[test]
    fn test_arch_key_counts() {
        // x86 PKU: 4-bit index × 2 nibbles = 16 keys.
        assert_eq!(ARCH_PKEY_COUNT_X86, 16);
        assert!(ARCH_PKEY_COUNT_X86.is_power_of_two());
        assert!(ARCH_PKEY_COUNT_X86 <= ARCH_PKEY_COUNT_MAX);
        // Default key 0 is reserved by the kernel for new mappings.
        assert_eq!(PKEY_DEFAULT, 0);
    }

    #[test]
    fn test_syscall_numbers_consecutive() {
        // pkey_mprotect / pkey_alloc / pkey_free were added together
        // and got consecutive syscall numbers 329, 330, 331.
        assert_eq!(NR_PKEY_MPROTECT, 329);
        assert_eq!(NR_PKEY_ALLOC, 330);
        assert_eq!(NR_PKEY_FREE, 331);
        assert_eq!(NR_PKEY_ALLOC, NR_PKEY_MPROTECT + 1);
        assert_eq!(NR_PKEY_FREE, NR_PKEY_ALLOC + 1);
    }

    #[test]
    fn test_prot_bits_dense_low_3() {
        // Same prot bits userspace passes to mprotect/mmap.
        assert_eq!(PROT_NONE, 0);
        assert!(PROT_READ.is_power_of_two());
        assert!(PROT_WRITE.is_power_of_two());
        assert!(PROT_EXEC.is_power_of_two());
        assert_eq!(PROT_READ | PROT_WRITE | PROT_EXEC, 0x7);
    }
}
