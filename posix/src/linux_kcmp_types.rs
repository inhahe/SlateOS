//! `<linux/kcmp.h>` — kcmp (kernel compare) syscall constants.
//!
//! The kcmp(2) syscall compares whether two processes share a
//! specific kernel resource (file descriptor, VM, filesystem info,
//! etc.). It was added for checkpoint/restore (CRIU) to determine
//! which resources are shared between processes so they can be
//! correctly saved and restored. Without kcmp, there's no way to
//! determine if two file descriptors in different processes point
//! to the same kernel struct file from userspace.

// ---------------------------------------------------------------------------
// kcmp resource types (KCMP_*)
// ---------------------------------------------------------------------------

/// Compare file descriptors (struct file pointer).
pub const KCMP_FILE: u32 = 0;
/// Compare VM (address space — struct mm_struct).
pub const KCMP_VM: u32 = 1;
/// Compare files struct (file descriptor table).
pub const KCMP_FILES: u32 = 2;
/// Compare filesystem info (root dir, umask, cwd).
pub const KCMP_FS: u32 = 3;
/// Compare signal handlers (struct sighand_struct).
pub const KCMP_SIGHAND: u32 = 4;
/// Compare I/O context.
pub const KCMP_IO: u32 = 5;
/// Compare sysvsem undo list.
pub const KCMP_SYSVSEM: u32 = 6;
/// Compare epoll target file descriptors.
pub const KCMP_EPOLL_TFD: u32 = 7;

// ---------------------------------------------------------------------------
// kcmp return values
// ---------------------------------------------------------------------------

/// Resources are equal (same kernel object).
pub const KCMP_ORDER_EQUAL: i32 = 0;
/// First resource sorts before second.
pub const KCMP_ORDER_LESS: i32 = -1;
/// First resource sorts after second.
pub const KCMP_ORDER_GREATER: i32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_types_distinct() {
        let types = [
            KCMP_FILE, KCMP_VM, KCMP_FILES, KCMP_FS,
            KCMP_SIGHAND, KCMP_IO, KCMP_SYSVSEM, KCMP_EPOLL_TFD,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_orders_distinct() {
        let orders = [KCMP_ORDER_EQUAL, KCMP_ORDER_LESS, KCMP_ORDER_GREATER];
        for i in 0..orders.len() {
            for j in (i + 1)..orders.len() {
                assert_ne!(orders[i], orders[j]);
            }
        }
    }

    #[test]
    fn test_order_equal_is_zero() {
        assert_eq!(KCMP_ORDER_EQUAL, 0);
    }

    #[test]
    fn test_resource_types_sequential() {
        assert_eq!(KCMP_FILE, 0);
        assert_eq!(KCMP_EPOLL_TFD, 7);
    }
}
