//! `<linux/sysctl.h>` — sysctl category and type constants.
//!
//! sysctl provides runtime-tunable kernel parameters exposed as files
//! under /proc/sys/. Categories organize parameters into subsystems
//! (kernel, vm, net, fs, etc.). Though the sysctl() syscall is
//! deprecated in favor of /proc/sys file access, the category numbers
//! and types remain relevant for binary compatibility.

// ---------------------------------------------------------------------------
// Top-level sysctl categories (CTL_*)
// ---------------------------------------------------------------------------

/// Kernel parameters.
pub const CTL_KERN: u32 = 1;
/// Virtual memory parameters.
pub const CTL_VM: u32 = 2;
/// Network parameters.
pub const CTL_NET: u32 = 3;
/// Filesystem parameters.
pub const CTL_FS: u32 = 5;
/// Device parameters.
pub const CTL_DEV: u32 = 7;
/// ABI emulation.
pub const CTL_ABI: u32 = 9;

// ---------------------------------------------------------------------------
// Kernel subcategories (KERN_*)
// ---------------------------------------------------------------------------

/// OS type string.
pub const KERN_OSTYPE: u32 = 1;
/// OS release string.
pub const KERN_OSRELEASE: u32 = 2;
/// OS revision.
pub const KERN_OSREV: u32 = 3;
/// Kernel version string.
pub const KERN_VERSION: u32 = 4;
/// Hostname.
pub const KERN_HOSTNAME: u32 = 10;
/// Domain name.
pub const KERN_DOMAINNAME: u32 = 12;
/// Maximum number of threads.
pub const KERN_MAX_THREADS: u32 = 33;
/// Panic timeout (seconds).
pub const KERN_PANIC: u32 = 15;
/// Random entropy pool.
pub const KERN_RANDOM: u32 = 44;
/// Maximum PID value.
pub const KERN_PID_MAX: u32 = 55;

// ---------------------------------------------------------------------------
// VM subcategories (VM_*)
// ---------------------------------------------------------------------------

/// Overcommit memory mode.
pub const VM_OVERCOMMIT_MEMORY: u32 = 5;
/// Swappiness (0-200).
pub const VM_SWAPPINESS: u32 = 19;
/// Dirty ratio (percent).
pub const VM_DIRTY_RATIO: u32 = 8;
/// Dirty background ratio (percent).
pub const VM_DIRTY_BACKGROUND: u32 = 9;
/// Minimum free kbytes.
pub const VM_MIN_FREE_KBYTES: u32 = 39;
/// OOM kill allocating task.
pub const VM_OOM_KILL: u32 = 24;
/// Compact memory.
pub const VM_COMPACT_MEMORY: u32 = 36;

// ---------------------------------------------------------------------------
// FS subcategories (FS_*)
// ---------------------------------------------------------------------------

/// Maximum number of open files (system-wide).
pub const FS_NRFILE: u32 = 6;
/// Maximum inotify instances per user.
pub const FS_INOTIFY: u32 = 13;
/// Maximum number of inodes.
pub const FS_MAXINODE: u32 = 7;
/// Lease break time (seconds).
pub const FS_LEASE_TIME: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_categories_distinct() {
        let cats = [CTL_KERN, CTL_VM, CTL_NET, CTL_FS, CTL_DEV, CTL_ABI];
        for i in 0..cats.len() {
            for j in (i + 1)..cats.len() {
                assert_ne!(cats[i], cats[j]);
            }
        }
    }

    #[test]
    fn test_kern_subcats_distinct() {
        let subs = [
            KERN_OSTYPE,
            KERN_OSRELEASE,
            KERN_OSREV,
            KERN_VERSION,
            KERN_HOSTNAME,
            KERN_DOMAINNAME,
            KERN_MAX_THREADS,
            KERN_PANIC,
            KERN_RANDOM,
            KERN_PID_MAX,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_vm_subcats_distinct() {
        let subs = [
            VM_OVERCOMMIT_MEMORY,
            VM_SWAPPINESS,
            VM_DIRTY_RATIO,
            VM_DIRTY_BACKGROUND,
            VM_MIN_FREE_KBYTES,
            VM_OOM_KILL,
            VM_COMPACT_MEMORY,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_fs_subcats_distinct() {
        let subs = [FS_NRFILE, FS_INOTIFY, FS_MAXINODE, FS_LEASE_TIME];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }
}
