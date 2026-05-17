//! `<linux/sched.h>` — CPU set (affinity mask) constants.
//!
//! CPU sets represent a set of CPUs as a bitmask. They are used with
//! sched_setaffinity/sched_getaffinity to control which CPUs a thread
//! may run on, and with cpuset cgroups for hierarchical CPU partitioning.
//! The kernel supports up to NR_CPUS (compile-time max), typically 8192.

// ---------------------------------------------------------------------------
// CPU set size constants
// ---------------------------------------------------------------------------

/// Maximum number of CPUs supported in a cpu_set_t.
pub const CPU_SETSIZE: u32 = 1024;
/// Number of bits per word in the CPU mask.
pub const NCPUBITS: u32 = 64;
/// Number of u64 words needed for CPU_SETSIZE CPUs.
pub const CPU_SET_WORDS: u32 = CPU_SETSIZE / NCPUBITS;

// ---------------------------------------------------------------------------
// CPU set operation helpers (shift/mask values)
// ---------------------------------------------------------------------------

/// Bit shift to find word index from CPU number.
pub const CPU_WORD_SHIFT: u32 = 6;
/// Mask to find bit position within a word.
pub const CPU_BIT_MASK: u32 = 0x3F;

// ---------------------------------------------------------------------------
// Kernel NR_CPUS configurations (common values)
// ---------------------------------------------------------------------------

/// Typical desktop kernel NR_CPUS.
pub const NR_CPUS_DESKTOP: u32 = 256;
/// Typical server kernel NR_CPUS.
pub const NR_CPUS_SERVER: u32 = 8192;
/// Minimum NR_CPUS.
pub const NR_CPUS_MIN: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_size_consistency() {
        assert_eq!(CPU_SET_WORDS, CPU_SETSIZE / NCPUBITS);
        assert_eq!(CPU_SET_WORDS, 16);
    }

    #[test]
    fn test_word_shift() {
        assert_eq!(1u32 << CPU_WORD_SHIFT, NCPUBITS);
    }

    #[test]
    fn test_bit_mask() {
        assert_eq!(CPU_BIT_MASK, NCPUBITS - 1);
    }

    #[test]
    fn test_nr_cpus_ordering() {
        assert!(NR_CPUS_MIN < NR_CPUS_DESKTOP);
        assert!(NR_CPUS_DESKTOP < NR_CPUS_SERVER);
    }

    #[test]
    fn test_setsize_power_of_two() {
        assert!(CPU_SETSIZE.is_power_of_two());
    }
}
