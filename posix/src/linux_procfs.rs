//! `<linux/proc_fs.h>` — procfs virtual filesystem constants.
//!
//! procfs (/proc) is the kernel's primary interface for exposing
//! process information and system statistics to userspace. Each
//! process gets a /proc/<pid> directory; system-wide information
//! lives in /proc/meminfo, /proc/cpuinfo, etc.

// ---------------------------------------------------------------------------
// procfs mount point
// ---------------------------------------------------------------------------

/// Default procfs mount point.
pub const PROCFS_MOUNT: &str = "/proc";

// ---------------------------------------------------------------------------
// Per-process file names
// ---------------------------------------------------------------------------

/// Process status.
pub const PROC_STATUS: &str = "status";
/// Process memory maps.
pub const PROC_MAPS: &str = "maps";
/// Process command line.
pub const PROC_CMDLINE: &str = "cmdline";
/// Process environment.
pub const PROC_ENVIRON: &str = "environ";
/// Process file descriptors directory.
pub const PROC_FD: &str = "fd";
/// Process stat (scheduling info).
pub const PROC_STAT: &str = "stat";
/// Process stat (human-readable).
pub const PROC_STATM: &str = "statm";
/// Process I/O statistics.
pub const PROC_IO: &str = "io";
/// Process CWD link.
pub const PROC_CWD: &str = "cwd";
/// Process executable link.
pub const PROC_EXE: &str = "exe";
/// Process root link.
pub const PROC_ROOT: &str = "root";
/// Process mount namespace info.
pub const PROC_MOUNTINFO: &str = "mountinfo";
/// Process cgroup membership.
pub const PROC_CGROUP: &str = "cgroup";
/// Process OOM score.
pub const PROC_OOM_SCORE: &str = "oom_score";
/// Process OOM score adjustment.
pub const PROC_OOM_SCORE_ADJ: &str = "oom_score_adj";

// ---------------------------------------------------------------------------
// System-wide file names
// ---------------------------------------------------------------------------

/// Memory info.
pub const PROC_MEMINFO: &str = "meminfo";
/// CPU info.
pub const PROC_CPUINFO: &str = "cpuinfo";
/// Load averages.
pub const PROC_LOADAVG: &str = "loadavg";
/// Uptime.
pub const PROC_UPTIME: &str = "uptime";
/// Kernel version.
pub const PROC_VERSION: &str = "version";
/// Filesystem types.
pub const PROC_FILESYSTEMS: &str = "filesystems";
/// Partition info.
pub const PROC_PARTITIONS: &str = "partitions";
/// Mounted filesystems.
pub const PROC_MOUNTS: &str = "mounts";
/// Disk I/O statistics.
pub const PROC_DISKSTATS: &str = "diskstats";
/// Swap info.
pub const PROC_SWAPS: &str = "swaps";
/// VM statistics.
pub const PROC_VMSTAT: &str = "vmstat";
/// Kernel command line.
pub const PROC_KERNEL_CMDLINE: &str = "cmdline";

// ---------------------------------------------------------------------------
// procfs directory names
// ---------------------------------------------------------------------------

/// System control.
pub const PROC_SYS: &str = "sys";
/// Network information.
pub const PROC_NET: &str = "net";
/// IRQ information.
pub const PROC_INTERRUPTS: &str = "interrupts";
/// Self (current process) symlink.
pub const PROC_SELF: &str = "self";
/// Thread self symlink.
pub const PROC_THREAD_SELF: &str = "thread-self";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_point() {
        assert_eq!(PROCFS_MOUNT, "/proc");
    }

    #[test]
    fn test_per_process_files_distinct() {
        let files = [
            PROC_STATUS,
            PROC_MAPS,
            PROC_CMDLINE,
            PROC_ENVIRON,
            PROC_FD,
            PROC_STAT,
            PROC_STATM,
            PROC_IO,
            PROC_CWD,
            PROC_EXE,
            PROC_ROOT,
            PROC_MOUNTINFO,
            PROC_CGROUP,
            PROC_OOM_SCORE,
            PROC_OOM_SCORE_ADJ,
        ];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }

    #[test]
    fn test_system_files_distinct() {
        let files = [
            PROC_MEMINFO,
            PROC_CPUINFO,
            PROC_LOADAVG,
            PROC_UPTIME,
            PROC_VERSION,
            PROC_FILESYSTEMS,
            PROC_PARTITIONS,
            PROC_MOUNTS,
            PROC_DISKSTATS,
            PROC_SWAPS,
            PROC_VMSTAT,
            PROC_KERNEL_CMDLINE,
        ];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }

    #[test]
    fn test_dirs_distinct() {
        let dirs = [
            PROC_SYS,
            PROC_NET,
            PROC_INTERRUPTS,
            PROC_SELF,
            PROC_THREAD_SELF,
        ];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }
}
