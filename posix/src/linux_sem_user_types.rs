//! `<sys/sem.h>` — System V semaphore ABI.
//!
//! `semget`/`semop`/`semctl` are the SysV IPC semaphore primitives.
//! Oracle/DB2, the JVM, the X server, and dozens of legacy daemons
//! still rely on them; `ipcs`/`ipcrm` are their userspace tools.
//! POSIX semaphores (`sem_open`/`sem_wait`) live in a different
//! ABI and aren't covered here.

// ---------------------------------------------------------------------------
// `semctl` commands
// ---------------------------------------------------------------------------

pub const GETPID: u32 = 11;
pub const GETVAL: u32 = 12;
pub const GETALL: u32 = 13;
pub const GETNCNT: u32 = 14;
pub const GETZCNT: u32 = 15;
pub const SETVAL: u32 = 16;
pub const SETALL: u32 = 17;

/// IPC-shared commands (`IPC_*`) — same values used by msq, sem, shm.
pub const IPC_RMID: u32 = 0;
pub const IPC_SET: u32 = 1;
pub const IPC_STAT: u32 = 2;
pub const IPC_INFO: u32 = 3;

// ---------------------------------------------------------------------------
// `semget` permission/flag bits — share the same bits as file modes
// ---------------------------------------------------------------------------

pub const IPC_CREAT: u32 = 0o1000;
pub const IPC_EXCL: u32 = 0o2000;
pub const IPC_NOWAIT: u32 = 0o4000;

// ---------------------------------------------------------------------------
// `semop` flag bits (`sembuf.sem_flg`)
// ---------------------------------------------------------------------------

pub const SEM_UNDO: u16 = 0x1000;

// ---------------------------------------------------------------------------
// Limits from `<linux/sem.h>` (`SEMMNI`, …)
// ---------------------------------------------------------------------------

/// Maximum number of semaphore sets system-wide.
pub const SEMMNI: u32 = 32_000;
/// Maximum semaphores per set.
pub const SEMMSL: u32 = 32_000;
/// Maximum semaphore ops in a single `semop(2)` call.
pub const SEMOPM: u32 = 500;
/// Maximum value for a semaphore.
pub const SEMVMX: u32 = 32_767;
/// Maximum adjust-on-exit value per semaphore.
pub const SEMAEM: u32 = SEMVMX;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_SEMGET: u32 = 64;
pub const NR_SEMOP: u32 = 65;
pub const NR_SEMCTL: u32 = 66;
pub const NR_SEMTIMEDOP: u32 = 220;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semctl_get_commands_dense_11_to_15() {
        assert_eq!(GETPID, 11);
        assert_eq!(GETVAL, 12);
        assert_eq!(GETALL, 13);
        assert_eq!(GETNCNT, 14);
        assert_eq!(GETZCNT, 15);
        // Followed by SETVAL/SETALL at 16/17 in the same dense block.
        assert_eq!(SETVAL, GETZCNT + 1);
        assert_eq!(SETALL, SETVAL + 1);
    }

    #[test]
    fn test_ipc_shared_commands_dense_0_to_3() {
        assert_eq!(IPC_RMID, 0);
        assert_eq!(IPC_SET, 1);
        assert_eq!(IPC_STAT, 2);
        assert_eq!(IPC_INFO, 3);
    }

    #[test]
    fn test_ipc_creat_excl_nowait_octal() {
        // The IPC_CREAT/EXCL/NOWAIT bits sit at 01000/02000/04000.
        // Single-bit, dense, just above the file-mode bits.
        assert!(IPC_CREAT.is_power_of_two());
        assert!(IPC_EXCL.is_power_of_two());
        assert!(IPC_NOWAIT.is_power_of_two());
        assert_eq!(IPC_EXCL, IPC_CREAT << 1);
        assert_eq!(IPC_NOWAIT, IPC_CREAT << 2);
        assert_eq!(IPC_CREAT | IPC_EXCL | IPC_NOWAIT, 0o7000);
    }

    #[test]
    fn test_sem_undo_value() {
        // SEM_UNDO is `0x1000` — same bit pattern as IPC_CREAT in
        // file-mode-bit ordering but used in a separate flag word.
        assert_eq!(SEM_UNDO, 0x1000);
    }

    #[test]
    fn test_limits_match_kernel_defaults() {
        assert_eq!(SEMMNI, 32_000);
        assert_eq!(SEMMSL, 32_000);
        assert_eq!(SEMOPM, 500);
        assert_eq!(SEMVMX, 32_767);
        assert_eq!(SEMAEM, SEMVMX);
        // SEMVMX < i16::MAX so values fit in a signed 16-bit field.
        assert!(SEMVMX <= i16::MAX as u32);
    }

    #[test]
    fn test_syscall_numbers_dense_block() {
        // semget/semop/semctl form a dense block at 64..=66.
        assert_eq!(NR_SEMOP, NR_SEMGET + 1);
        assert_eq!(NR_SEMCTL, NR_SEMOP + 1);
        // semtimedop was added later at 220.
        assert_eq!(NR_SEMTIMEDOP, 220);
    }
}
