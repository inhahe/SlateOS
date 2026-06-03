//! `<linux/kcmp.h>` — `kcmp(2)` syscall constants.
//!
//! `kcmp(pid1, pid2, type, idx1, idx2)` lets userspace ask the kernel
//! whether two kernel objects in two different processes are the same
//! underlying object. CRIU uses this heavily during checkpoint/restore
//! to detect shared FDs, shared address spaces, shared signal handlers,
//! and shared SysV semaphore undo lists.

// ---------------------------------------------------------------------------
// Syscall number on x86_64
// ---------------------------------------------------------------------------

/// `__NR_kcmp` on x86_64. Available since Linux 3.5.
pub const NR_KCMP: u32 = 312;

// ---------------------------------------------------------------------------
// `enum kcmp_type` — what to compare
// ---------------------------------------------------------------------------

/// Same `struct file *` (open file description).
pub const KCMP_FILE: u32 = 0;
/// Same `struct mm_struct` (address space).
pub const KCMP_VM: u32 = 1;
/// Same `struct files_struct` (FD table).
pub const KCMP_FILES: u32 = 2;
/// Same `struct fs_struct` (cwd, root, umask).
pub const KCMP_FS: u32 = 3;
/// Same `struct sighand_struct` (signal handlers).
pub const KCMP_SIGHAND: u32 = 4;
/// Same `struct io_context` (block I/O context).
pub const KCMP_IO: u32 = 5;
/// Same SysV semaphore undo list.
pub const KCMP_SYSVSEM: u32 = 6;
/// Same `epoll(7)` target-file description (Linux 4.13+).
pub const KCMP_EPOLL_TFD: u32 = 7;
/// Sentinel — one past the last defined type.
pub const KCMP_TYPES: u32 = 8;

// ---------------------------------------------------------------------------
// kcmp return-value semantics
// ---------------------------------------------------------------------------

/// Returned when the two kernel objects are equal.
pub const KCMP_EQUAL: i32 = 0;
/// Returned when `idx1`'s object orders before `idx2`'s.
pub const KCMP_LT: i32 = 1;
/// Returned when `idx1`'s object orders after `idx2`'s.
pub const KCMP_GT: i32 = 2;
/// Returned when the objects are distinct but ordering can't be expressed.
pub const KCMP_DIFFER: i32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syscall_number_x86_64() {
        // __NR_kcmp on x86_64 has been 312 since Linux 3.5.
        assert_eq!(NR_KCMP, 312);
    }

    #[test]
    fn test_types_dense_0_to_7_with_sentinel() {
        let t = [
            KCMP_FILE,
            KCMP_VM,
            KCMP_FILES,
            KCMP_FS,
            KCMP_SIGHAND,
            KCMP_IO,
            KCMP_SYSVSEM,
            KCMP_EPOLL_TFD,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // Sentinel is one past the last enumerator.
        assert_eq!(KCMP_TYPES as usize, t.len());
    }

    #[test]
    fn test_return_values_distinct_and_dense() {
        let r = [KCMP_EQUAL, KCMP_LT, KCMP_GT, KCMP_DIFFER];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}
