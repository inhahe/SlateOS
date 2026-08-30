//! `<sched.h>` — `cpu_set_t` bitmap macros (CPU_SET, CPU_CLR, CPU_ISSET,
//! CPU_ZERO, CPU_COUNT).
//!
//! These constants describe the layout of the bitmap so callers can
//! implement the macros without pulling in glibc-specific headers.

// ---------------------------------------------------------------------------
// Bitmap dimensions
// ---------------------------------------------------------------------------

/// Total CPUs in a static `cpu_set_t` (glibc default = 1024).
pub const CPU_SETSIZE: u32 = 1024;
/// Bits in one underlying word.
pub const NCPUBITS: u32 = 64;
/// Word count: ceil(CPU_SETSIZE / NCPUBITS) = 16.
pub const CPU_WORDS: u32 = CPU_SETSIZE / NCPUBITS;
/// Total byte size.
pub const CPU_SET_BYTES: usize = (CPU_WORDS * 8) as usize;

// ---------------------------------------------------------------------------
// Helper masks
// ---------------------------------------------------------------------------

/// Mask of the low bits within one word (NCPUBITS - 1).
pub const NCPUBITS_MASK: u64 = (NCPUBITS as u64) - 1;
/// Shift amount to go from a CPU index to a word index.
pub const NCPUBITS_SHIFT: u32 = NCPUBITS.trailing_zeros();

// ---------------------------------------------------------------------------
// Errors from sched_setaffinity / sched_getaffinity
// ---------------------------------------------------------------------------

pub const CPU_SET_EFAULT: i32 = 14;
pub const CPU_SET_EINVAL: i32 = 22;
pub const CPU_SET_EPERM: i32 = 1;
pub const CPU_SET_ESRCH: i32 = 3;

// ---------------------------------------------------------------------------
// Maximum CPUs the kernel supports (CONFIG_NR_CPUS upper bound)
// ---------------------------------------------------------------------------

/// Upper bound used by mainline kernels with default x86_64 config.
pub const NR_CPUS_MAX: u32 = 8192;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geometry_matches_glibc_default() {
        assert_eq!(CPU_SETSIZE, 1024);
        assert_eq!(NCPUBITS, 64);
        assert_eq!(CPU_WORDS, 16);
        assert_eq!(CPU_SET_BYTES, 128);
    }

    #[test]
    fn test_ncpubits_mask_and_shift_match() {
        assert_eq!(NCPUBITS_MASK, 63);
        assert_eq!(NCPUBITS_SHIFT, 6);
        // shift+mask reconstructs index decomposition: cpu = word * 64 + bit.
        assert_eq!(1u64 << NCPUBITS_SHIFT, NCPUBITS as u64);
        assert_eq!((NCPUBITS as u64) - 1, NCPUBITS_MASK);
    }

    #[test]
    fn test_total_bits_equals_cpu_setsize() {
        assert_eq!(CPU_WORDS * NCPUBITS, CPU_SETSIZE);
    }

    #[test]
    fn test_errnos_distinct_standard() {
        let e = [CPU_SET_EFAULT, CPU_SET_EINVAL, CPU_SET_EPERM, CPU_SET_ESRCH];
        for (i, &x) in e.iter().enumerate() {
            for &y in &e[i + 1..] {
                assert_ne!(x, y);
            }
        }
        assert_eq!(CPU_SET_EPERM, 1);
        assert_eq!(CPU_SET_ESRCH, 3);
        assert_eq!(CPU_SET_EFAULT, 14);
        assert_eq!(CPU_SET_EINVAL, 22);
    }

    #[test]
    fn test_nr_cpus_max_is_8192() {
        assert_eq!(NR_CPUS_MAX, 8192);
        assert!(NR_CPUS_MAX.is_power_of_two());
        // Statically allocated cpu_set_t holds fewer.
        assert!(CPU_SETSIZE < NR_CPUS_MAX);
    }
}
