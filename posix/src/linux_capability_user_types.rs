//! `<linux/capability.h>` — POSIX.1e capability constants for userspace.
//!
//! Linux capabilities split root privilege into ~40 distinct bits.
//! libcap, systemd, container runtimes, and setcap/getcap all read
//! the constants below to map names to bit positions in the
//! permitted/effective/inheritable capability sets.

// ---------------------------------------------------------------------------
// capget/capset header version
// ---------------------------------------------------------------------------

/// Original (broken) capability ABI — single 32-bit set.
pub const LINUX_CAPABILITY_VERSION_1: u32 = 0x1998_0330;
/// Second revision — two 32-bit sets, deprecated.
pub const LINUX_CAPABILITY_VERSION_2: u32 = 0x2007_1026;
/// Current ABI — two 32-bit sets with file-cap v3 semantics.
pub const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;

/// Number of u32 slots in the v3 capability header.
pub const LINUX_CAPABILITY_U32S_3: u32 = 2;

// ---------------------------------------------------------------------------
// Capability bit numbers (CAP_*)
// ---------------------------------------------------------------------------

/// Override file owner restrictions on chown(2).
pub const CAP_CHOWN: u32 = 0;
/// Bypass DAC read/write/execute permission checks.
pub const CAP_DAC_OVERRIDE: u32 = 1;
/// Bypass DAC read/search permission checks.
pub const CAP_DAC_READ_SEARCH: u32 = 2;
/// Bypass file-owner permission checks.
pub const CAP_FOWNER: u32 = 3;
/// Don't clear setuid/setgid bits on file modifications.
pub const CAP_FSETID: u32 = 4;
/// Bypass kill(2) permission checks.
pub const CAP_KILL: u32 = 5;
/// Change real/effective/saved GIDs and the supplementary GID list.
pub const CAP_SETGID: u32 = 6;
/// Change real/effective/saved UIDs.
pub const CAP_SETUID: u32 = 7;
/// Set capabilities on files and processes.
pub const CAP_SETPCAP: u32 = 8;
/// Allow modification of S_IMMUTABLE / S_APPEND flags.
pub const CAP_LINUX_IMMUTABLE: u32 = 9;
/// Bind to privileged ports (<1024).
pub const CAP_NET_BIND_SERVICE: u32 = 10;
/// Allow broadcast / multicast listening.
pub const CAP_NET_BROADCAST: u32 = 11;
/// Configure interfaces, routing, firewall, raw sockets.
pub const CAP_NET_ADMIN: u32 = 12;
/// Use RAW and PACKET sockets.
pub const CAP_NET_RAW: u32 = 13;
/// Override IPC ownership checks; lock memory.
pub const CAP_IPC_LOCK: u32 = 14;
/// Bypass IPC permission checks.
pub const CAP_IPC_OWNER: u32 = 15;
/// Load and unload kernel modules.
pub const CAP_SYS_MODULE: u32 = 16;
/// Perform raw I/O (ioperm/iopl).
pub const CAP_SYS_RAWIO: u32 = 17;
/// chroot(2).
pub const CAP_SYS_CHROOT: u32 = 18;
/// ptrace(2) arbitrary processes.
pub const CAP_SYS_PTRACE: u32 = 19;
/// Configure process accounting.
pub const CAP_SYS_PACCT: u32 = 20;
/// General system administration (mount, swapon, hostname, ...).
pub const CAP_SYS_ADMIN: u32 = 21;
/// Reboot the system.
pub const CAP_SYS_BOOT: u32 = 22;
/// Set scheduling policy / priority for arbitrary tasks.
pub const CAP_SYS_NICE: u32 = 23;
/// Override resource limits.
pub const CAP_SYS_RESOURCE: u32 = 24;
/// Set system clock and real-time clock.
pub const CAP_SYS_TIME: u32 = 25;
/// Configure tty devices, vhangup.
pub const CAP_SYS_TTY_CONFIG: u32 = 26;
/// Make arbitrary device-special files.
pub const CAP_MKNOD: u32 = 27;
/// Take file leases.
pub const CAP_LEASE: u32 = 28;
/// Write to the kernel audit log.
pub const CAP_AUDIT_WRITE: u32 = 29;
/// Configure the kernel audit subsystem.
pub const CAP_AUDIT_CONTROL: u32 = 30;
/// Set file capabilities.
pub const CAP_SETFCAP: u32 = 31;
/// Override MAC checks.
pub const CAP_MAC_OVERRIDE: u32 = 32;
/// Configure MAC policy.
pub const CAP_MAC_ADMIN: u32 = 33;
/// Configure kernel syslog (printk).
pub const CAP_SYSLOG: u32 = 34;
/// Trigger wake-up events on suspended devices.
pub const CAP_WAKE_ALARM: u32 = 35;
/// Block system suspend.
pub const CAP_BLOCK_SUSPEND: u32 = 36;
/// Read kernel audit log.
pub const CAP_AUDIT_READ: u32 = 37;
/// Perform privileged perf_event operations.
pub const CAP_PERFMON: u32 = 38;
/// Privileged BPF operations.
pub const CAP_BPF: u32 = 39;
/// Checkpoint/restore arbitrary tasks (CRIU).
pub const CAP_CHECKPOINT_RESTORE: u32 = 40;

/// Highest defined capability number.
pub const CAP_LAST_CAP: u32 = CAP_CHECKPOINT_RESTORE;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_distinct_and_dated() {
        // Each ABI version is the date the spec stabilised; they must
        // remain distinct so capget(2) can refuse the wrong layout.
        assert_ne!(LINUX_CAPABILITY_VERSION_1, LINUX_CAPABILITY_VERSION_2);
        assert_ne!(LINUX_CAPABILITY_VERSION_2, LINUX_CAPABILITY_VERSION_3);
        assert_ne!(LINUX_CAPABILITY_VERSION_1, LINUX_CAPABILITY_VERSION_3);
        // v3 has exactly two u32 slots (so caps 32..63 live in the
        // second word).
        assert_eq!(LINUX_CAPABILITY_U32S_3, 2);
    }

    #[test]
    fn test_caps_dense_and_ordered() {
        let caps = [
            CAP_CHOWN,
            CAP_DAC_OVERRIDE,
            CAP_DAC_READ_SEARCH,
            CAP_FOWNER,
            CAP_FSETID,
            CAP_KILL,
            CAP_SETGID,
            CAP_SETUID,
            CAP_SETPCAP,
            CAP_LINUX_IMMUTABLE,
            CAP_NET_BIND_SERVICE,
            CAP_NET_BROADCAST,
            CAP_NET_ADMIN,
            CAP_NET_RAW,
            CAP_IPC_LOCK,
            CAP_IPC_OWNER,
            CAP_SYS_MODULE,
            CAP_SYS_RAWIO,
            CAP_SYS_CHROOT,
            CAP_SYS_PTRACE,
            CAP_SYS_PACCT,
            CAP_SYS_ADMIN,
            CAP_SYS_BOOT,
            CAP_SYS_NICE,
            CAP_SYS_RESOURCE,
            CAP_SYS_TIME,
            CAP_SYS_TTY_CONFIG,
            CAP_MKNOD,
            CAP_LEASE,
            CAP_AUDIT_WRITE,
            CAP_AUDIT_CONTROL,
            CAP_SETFCAP,
            CAP_MAC_OVERRIDE,
            CAP_MAC_ADMIN,
            CAP_SYSLOG,
            CAP_WAKE_ALARM,
            CAP_BLOCK_SUSPEND,
            CAP_AUDIT_READ,
            CAP_PERFMON,
            CAP_BPF,
            CAP_CHECKPOINT_RESTORE,
        ];
        // Capability numbers must be a dense, monotonically increasing
        // sequence starting at 0 — capget/capset uses each as a bit
        // index, so a hole would silently shift everything after it.
        for (i, &c) in caps.iter().enumerate() {
            assert_eq!(c as usize, i);
        }
        assert_eq!(CAP_LAST_CAP, 40);
    }

    #[test]
    fn test_last_cap_fits_in_two_u32s() {
        // The v3 ABI provides 64 bits; CAP_LAST_CAP must fit.
        assert!(CAP_LAST_CAP < 64);
    }
}
