//! `<linux/sched.h>` (affinity subset) — CPU affinity constants.
//!
//! CPU affinity determines which CPUs a thread is allowed to run on.
//! The affinity mask is a bitmask where each bit represents a CPU.
//! sched_setaffinity()/sched_getaffinity() control per-thread masks.
//! Affinity is used for performance (pin threads to CPUs to improve
//! cache locality), isolation (keep latency-sensitive tasks on
//! dedicated CPUs), and NUMA optimization (keep threads near their
//! memory).

// ---------------------------------------------------------------------------
// CPU set size limits
// ---------------------------------------------------------------------------

/// Maximum number of CPUs supported (NR_CPUS typical default).
pub const CPU_SETSIZE: u32 = 1024;
/// Bits per CPU mask word.
pub const CPU_MASK_BITS: u32 = 64;
/// Number of words in a maximum-sized CPU set.
pub const CPU_MASK_WORDS: u32 = CPU_SETSIZE / CPU_MASK_BITS;

// ---------------------------------------------------------------------------
// Affinity flags (for sched_setaffinity extensions)
// ---------------------------------------------------------------------------

/// Normal affinity (allow migration between allowed CPUs).
pub const SCHED_AFFINITY_NORMAL: u32 = 0;
/// Prefer specified CPUs but allow migration if all are busy.
pub const SCHED_AFFINITY_PREFER: u32 = 1;
/// Strict affinity (never migrate off allowed CPUs).
pub const SCHED_AFFINITY_STRICT: u32 = 2;

// ---------------------------------------------------------------------------
// NUMA node affinity
// ---------------------------------------------------------------------------

/// No preferred NUMA node (kernel chooses).
pub const NUMA_NO_NODE: i32 = -1;
/// Maximum NUMA nodes supported.
pub const MAX_NUMNODES: u32 = 1024;

// ---------------------------------------------------------------------------
// CPU isolation flags (isolcpus kernel parameter)
// ---------------------------------------------------------------------------

/// Isolate from general SMP balancing.
pub const CPU_ISOLATE_DOMAIN: u32 = 0x01;
/// Isolate from managed IRQs.
pub const CPU_ISOLATE_MANAGED_IRQ: u32 = 0x02;
/// Isolate from unbound workqueues.
pub const CPU_ISOLATE_NOHZ: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_set_math() {
        assert_eq!(CPU_MASK_WORDS, 16);
        assert_eq!(CPU_SETSIZE, CPU_MASK_WORDS * CPU_MASK_BITS);
    }

    #[test]
    fn test_affinity_modes_distinct() {
        let modes = [
            SCHED_AFFINITY_NORMAL,
            SCHED_AFFINITY_PREFER,
            SCHED_AFFINITY_STRICT,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_numa_no_node() {
        assert_eq!(NUMA_NO_NODE, -1);
        assert!(MAX_NUMNODES > 0);
    }

    #[test]
    fn test_isolate_flags_no_overlap() {
        let flags = [
            CPU_ISOLATE_DOMAIN,
            CPU_ISOLATE_MANAGED_IRQ,
            CPU_ISOLATE_NOHZ,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
