//! `<linux/ipc.h>` — System V IPC primitives (msgget/semget/shmget).
//!
//! Despite POSIX message queues and shared memory being more modern,
//! a huge body of Oracle, SAP, and PostgreSQL deployments still uses
//! the legacy `IPC_*` System V interface. `ipcs(1)`, `ipcrm(1)`, and
//! every `shmget(IPC_PRIVATE,…)` call walks through the constants
//! defined here.

// ---------------------------------------------------------------------------
// Common keys
// ---------------------------------------------------------------------------

/// `IPC_PRIVATE` — anonymous key, always returns a fresh object.
pub const IPC_PRIVATE: u32 = 0;

// ---------------------------------------------------------------------------
// Mode flags (or'd into msgflg / semflg / shmflg)
// ---------------------------------------------------------------------------

/// Create if non-existent.
pub const IPC_CREAT: u32 = 0o0001000;
/// Fail if key exists.
pub const IPC_EXCL: u32 = 0o0002000;
/// Return EAGAIN instead of blocking.
pub const IPC_NOWAIT: u32 = 0o0004000;

// ---------------------------------------------------------------------------
// Control commands
// ---------------------------------------------------------------------------

/// Remove identifier.
pub const IPC_RMID: u32 = 0;
/// Set ipc_perm.
pub const IPC_SET: u32 = 1;
/// Get ipc_perm.
pub const IPC_STAT: u32 = 2;
/// Get info (kernel-internal).
pub const IPC_INFO: u32 = 3;

// ---------------------------------------------------------------------------
// Permissions encoding (low 9 bits of mode)
// ---------------------------------------------------------------------------

/// Read by owner.
pub const S_IRUSR: u32 = 0o400;
/// Write by owner.
pub const S_IWUSR: u32 = 0o200;
/// Read by group.
pub const S_IRGRP: u32 = 0o040;
/// Write by group.
pub const S_IWGRP: u32 = 0o020;
/// Read by other.
pub const S_IROTH: u32 = 0o004;
/// Write by other.
pub const S_IWOTH: u32 = 0o002;

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// Maximum message-text length (kernel default; tunable in /proc).
pub const MSGMAX: u32 = 8192;
/// Default maximum bytes in a message queue.
pub const MSGMNB: u32 = 16384;
/// Maximum number of semaphores per set.
pub const SEMMSL: u32 = 250;
/// Default minimum shared-memory segment size.
pub const SHMMIN: u32 = 1;

// ---------------------------------------------------------------------------
// `shmat` flags
// ---------------------------------------------------------------------------

pub const SHM_RDONLY: u32 = 0o010_000;
pub const SHM_RND: u32 = 0o020_000;
pub const SHM_REMAP: u32 = 0o040_000;
pub const SHM_EXEC: u32 = 0o100_000;

// ---------------------------------------------------------------------------
// `shmctl` extra commands
// ---------------------------------------------------------------------------

pub const SHM_LOCK: u32 = 11;
pub const SHM_UNLOCK: u32 = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_private_key_is_zero() {
        // IPC_PRIVATE == 0 is hard-coded in glibc and the kernel.
        assert_eq!(IPC_PRIVATE, 0);
    }

    #[test]
    fn test_mode_flags_distinct_and_pow2() {
        for &b in &[IPC_CREAT, IPC_EXCL, IPC_NOWAIT] {
            assert!(b.is_power_of_two());
        }
        assert_ne!(IPC_CREAT, IPC_EXCL);
        assert_ne!(IPC_EXCL, IPC_NOWAIT);
    }

    #[test]
    fn test_ctrl_commands_dense_0_to_3() {
        assert_eq!(IPC_RMID, 0);
        assert_eq!(IPC_SET, 1);
        assert_eq!(IPC_STAT, 2);
        assert_eq!(IPC_INFO, 3);
    }

    #[test]
    fn test_permissions_layout() {
        // User triplet = 0o600, group = 0o060, other = 0o006.
        assert_eq!(S_IRUSR | S_IWUSR, 0o600);
        assert_eq!(S_IRGRP | S_IWGRP, 0o060);
        assert_eq!(S_IROTH | S_IWOTH, 0o006);
    }

    #[test]
    fn test_size_constants_sane() {
        assert!(MSGMAX <= MSGMNB);
        assert!(SHMMIN >= 1);
        // 250 is the historical Linux SEMMSL default.
        assert_eq!(SEMMSL, 250);
    }

    #[test]
    fn test_shmat_flags_distinct() {
        for &b in &[SHM_RDONLY, SHM_RND, SHM_REMAP, SHM_EXEC] {
            assert!(b.is_power_of_two());
        }
        // Don't collide with the mode permission bits.
        assert_eq!(SHM_RDONLY & 0o777, 0);
    }

    #[test]
    fn test_shm_lock_codes() {
        assert_ne!(SHM_LOCK, SHM_UNLOCK);
        assert_eq!(SHM_UNLOCK, SHM_LOCK + 1);
    }
}
