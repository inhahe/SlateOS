//! `<sched.h>` — CPU affinity set constants and macros.
//!
//! CPU sets (`cpu_set_t`) are bitmasks used to restrict which CPUs
//! a thread or process may run on.  These constants define the
//! set capacity and helper values for manipulating CPU bits.

// ---------------------------------------------------------------------------
// CPU set capacity
// ---------------------------------------------------------------------------

/// Maximum number of CPUs in a standard cpu_set_t.
pub const CPU_SETSIZE: u32 = 1024;
/// Number of bits per word in the CPU set mask.
pub const CPU_BITS_PER_WORD: u32 = 64;
/// Number of words in a standard cpu_set_t.
pub const CPU_SET_WORDS: u32 = CPU_SETSIZE / CPU_BITS_PER_WORD;

// ---------------------------------------------------------------------------
// CPU set manipulation helpers (values for bitwise operations)
// ---------------------------------------------------------------------------

/// Bit mask for extracting the bit index within a word.
pub const CPU_BIT_MASK: u32 = CPU_BITS_PER_WORD - 1;
/// Shift amount to convert CPU number to word index.
pub const CPU_WORD_SHIFT: u32 = 6; // log2(64)

// ---------------------------------------------------------------------------
// Dynamic CPU set allocation constants
// ---------------------------------------------------------------------------

/// Minimum allocation size for dynamically-allocated CPU sets (bytes).
pub const CPU_ALLOC_MIN_BYTES: u32 = 8;
/// Alignment requirement for CPU set allocations (bytes).
pub const CPU_ALLOC_ALIGN: u32 = 8;

// ---------------------------------------------------------------------------
// sched_setaffinity / sched_getaffinity constants
// ---------------------------------------------------------------------------

/// Default size parameter for affinity syscalls (bytes for 1024 CPUs).
pub const CPU_AFFINITY_SIZE_DEFAULT: u32 = CPU_SETSIZE / 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setsize() {
        assert_eq!(CPU_SETSIZE, 1024);
    }

    #[test]
    fn test_bits_per_word() {
        assert_eq!(CPU_BITS_PER_WORD, 64);
    }

    #[test]
    fn test_set_words() {
        assert_eq!(CPU_SET_WORDS, 16);
        assert_eq!(CPU_SET_WORDS, CPU_SETSIZE / CPU_BITS_PER_WORD);
    }

    #[test]
    fn test_bit_mask() {
        assert_eq!(CPU_BIT_MASK, 63);
        assert_eq!(CPU_BIT_MASK, CPU_BITS_PER_WORD - 1);
    }

    #[test]
    fn test_word_shift() {
        assert_eq!(CPU_WORD_SHIFT, 6);
        assert_eq!(1u32 << CPU_WORD_SHIFT, CPU_BITS_PER_WORD);
    }

    #[test]
    fn test_alloc_min() {
        assert_eq!(CPU_ALLOC_MIN_BYTES, 8);
    }

    #[test]
    fn test_alloc_align() {
        assert!(CPU_ALLOC_ALIGN.is_power_of_two());
    }

    #[test]
    fn test_affinity_size_default() {
        assert_eq!(CPU_AFFINITY_SIZE_DEFAULT, 128);
        assert_eq!(CPU_AFFINITY_SIZE_DEFAULT, CPU_SETSIZE / 8);
    }

    #[test]
    fn test_setsize_is_power_of_two() {
        assert!(CPU_SETSIZE.is_power_of_two());
    }

    #[test]
    fn test_bits_per_word_is_power_of_two() {
        assert!(CPU_BITS_PER_WORD.is_power_of_two());
    }
}
