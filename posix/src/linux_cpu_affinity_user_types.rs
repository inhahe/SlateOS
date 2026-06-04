//! `<sched.h>` — CPU affinity API (sched_setaffinity, cpu_set_t).
//!
//! Linux's CPU affinity uses a bitmap of CPU indices. The classic
//! `cpu_set_t` is 1024 bits; modern glibc also exposes dynamic
//! variants via `CPU_ALLOC`. Each set bit means "this thread may
//! run on that logical CPU".

// ---------------------------------------------------------------------------
// Fixed cpu_set_t size (glibc default)
// ---------------------------------------------------------------------------

/// Total CPUs representable by a static `cpu_set_t`.
pub const CPU_SETSIZE: u32 = 1024;

/// Size of each underlying word in the bitmap (an unsigned long).
pub const NCPUBITS: u32 = 64;

/// Number of 64-bit words backing a static `cpu_set_t` (1024/64 = 16).
pub const CPU_SET_WORDS: usize = (CPU_SETSIZE / NCPUBITS) as usize;

/// Total byte size of a `cpu_set_t` (128 bytes).
pub const CPU_SET_SIZE_BYTES: usize = CPU_SET_WORDS * 8;

// ---------------------------------------------------------------------------
// Syscall numbers for sched_setaffinity / getaffinity
// ---------------------------------------------------------------------------

pub const NR_SCHED_SETAFFINITY_X86_64: u32 = 203;
pub const NR_SCHED_GETAFFINITY_X86_64: u32 = 204;
pub const NR_SCHED_SETAFFINITY_AARCH64: u32 = 122;
pub const NR_SCHED_GETAFFINITY_AARCH64: u32 = 123;
pub const NR_SCHED_SETAFFINITY_I386: u32 = 241;
pub const NR_SCHED_GETAFFINITY_I386: u32 = 242;

// ---------------------------------------------------------------------------
// /proc/<pid>/status field that exposes the current mask
// ---------------------------------------------------------------------------

pub const PROC_STATUS_CPUS_ALLOWED: &str = "Cpus_allowed";
pub const PROC_STATUS_CPUS_ALLOWED_LIST: &str = "Cpus_allowed_list";

// ---------------------------------------------------------------------------
// /sys files for online/offline/possible CPUs
// ---------------------------------------------------------------------------

pub const SYS_DEVICES_SYSTEM_CPU: &str = "/sys/devices/system/cpu";
pub const SYS_CPU_ONLINE: &str = "/sys/devices/system/cpu/online";
pub const SYS_CPU_OFFLINE: &str = "/sys/devices/system/cpu/offline";
pub const SYS_CPU_POSSIBLE: &str = "/sys/devices/system/cpu/possible";
pub const SYS_CPU_PRESENT: &str = "/sys/devices/system/cpu/present";

// ---------------------------------------------------------------------------
// Errno values commonly returned by sched_setaffinity
// ---------------------------------------------------------------------------

/// EFAULT — bad mask pointer.
pub const AFFINITY_EFAULT: i32 = 14;
/// EINVAL — mask doesn't intersect cpuset/policy.
pub const AFFINITY_EINVAL: i32 = 22;
/// EPERM — caller lacks CAP_SYS_NICE for foreign PID.
pub const AFFINITY_EPERM: i32 = 1;
/// ESRCH — target PID doesn't exist.
pub const AFFINITY_ESRCH: i32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_setsize_is_1024() {
        assert_eq!(CPU_SETSIZE, 1024);
        assert!(CPU_SETSIZE.is_power_of_two());
    }

    #[test]
    fn test_cpu_set_geometry() {
        assert_eq!(NCPUBITS, 64);
        assert_eq!(CPU_SET_WORDS, 16);
        assert_eq!(CPU_SET_SIZE_BYTES, 128);
        // word_count * bits_per_word == cpu_set bits.
        assert_eq!((CPU_SET_WORDS as u32) * NCPUBITS, CPU_SETSIZE);
    }

    #[test]
    fn test_syscall_numbers_paired() {
        // get == set + 1 on every architecture.
        assert_eq!(NR_SCHED_GETAFFINITY_X86_64, NR_SCHED_SETAFFINITY_X86_64 + 1);
        assert_eq!(NR_SCHED_GETAFFINITY_AARCH64, NR_SCHED_SETAFFINITY_AARCH64 + 1);
        assert_eq!(NR_SCHED_GETAFFINITY_I386, NR_SCHED_SETAFFINITY_I386 + 1);
    }

    #[test]
    fn test_syscall_numbers_per_arch_distinct() {
        let n = [
            NR_SCHED_SETAFFINITY_X86_64,
            NR_SCHED_SETAFFINITY_AARCH64,
            NR_SCHED_SETAFFINITY_I386,
        ];
        for (i, &x) in n.iter().enumerate() {
            for &y in &n[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_proc_status_field_names() {
        assert_eq!(PROC_STATUS_CPUS_ALLOWED, "Cpus_allowed");
        assert!(PROC_STATUS_CPUS_ALLOWED_LIST.starts_with(PROC_STATUS_CPUS_ALLOWED));
    }

    #[test]
    fn test_sys_cpu_paths_under_devices_system_cpu() {
        for p in [SYS_CPU_ONLINE, SYS_CPU_OFFLINE, SYS_CPU_POSSIBLE, SYS_CPU_PRESENT] {
            assert!(p.starts_with(SYS_DEVICES_SYSTEM_CPU));
        }
    }

    #[test]
    fn test_errno_values_distinct_and_classic() {
        let e = [
            AFFINITY_EFAULT,
            AFFINITY_EINVAL,
            AFFINITY_EPERM,
            AFFINITY_ESRCH,
        ];
        for (i, &x) in e.iter().enumerate() {
            for &y in &e[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // Standard generic-arch errno values.
        assert_eq!(AFFINITY_EPERM, 1);
        assert_eq!(AFFINITY_ESRCH, 3);
        assert_eq!(AFFINITY_EFAULT, 14);
        assert_eq!(AFFINITY_EINVAL, 22);
    }
}
