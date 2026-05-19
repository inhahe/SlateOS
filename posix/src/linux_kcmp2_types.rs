//! `<linux/kcmp.h>` — Additional kcmp constants.
//!
//! Supplementary kcmp constants covering comparison types,
//! result values, and epoll slot encoding.

// ---------------------------------------------------------------------------
// kcmp comparison types (KCMP_*)
// ---------------------------------------------------------------------------

/// Compare file descriptors.
pub const KCMP_FILE: u32 = 0;
/// Compare VM (address space).
pub const KCMP_VM: u32 = 1;
/// Compare filesystem info.
pub const KCMP_FILES: u32 = 2;
/// Compare filesystem root.
pub const KCMP_FS: u32 = 3;
/// Compare signal handlers.
pub const KCMP_SIGHAND: u32 = 4;
/// Compare I/O context.
pub const KCMP_IO: u32 = 5;
/// Compare sysvsem undo list.
pub const KCMP_SYSVSEM: u32 = 6;
/// Compare epoll target.
pub const KCMP_EPOLL_TFD: u32 = 7;

/// Number of kcmp types.
pub const KCMP_TYPES: u32 = 8;

// ---------------------------------------------------------------------------
// kcmp result values
// ---------------------------------------------------------------------------

/// Resources are equal.
pub const KCMP_EQ: i32 = 0;
/// First resource is less than second.
pub const KCMP_LT: i32 = 1;
/// First resource is greater than second.
pub const KCMP_GT: i32 = 2;

// ---------------------------------------------------------------------------
// kcmp epoll slot encoding
// ---------------------------------------------------------------------------

/// Epoll slot: file descriptor field offset.
pub const KCMP_EPOLL_TFD_FD_OFFSET: u32 = 0;
/// Epoll slot: target file offset.
pub const KCMP_EPOLL_TFD_TOFF_OFFSET: u32 = 4;
/// Epoll slot: target fd offset.
pub const KCMP_EPOLL_TFD_TFD_OFFSET: u32 = 8;
/// Size of kcmp_epoll_slot structure.
pub const KCMP_EPOLL_SLOT_SIZE: u32 = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
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
    fn test_types_count() {
        assert_eq!(KCMP_TYPES, 8);
        assert_eq!(KCMP_EPOLL_TFD + 1, KCMP_TYPES);
    }

    #[test]
    fn test_results_distinct() {
        let results = [KCMP_EQ, KCMP_LT, KCMP_GT];
        for i in 0..results.len() {
            for j in (i + 1)..results.len() {
                assert_ne!(results[i], results[j]);
            }
        }
    }

    #[test]
    fn test_epoll_offsets_increasing() {
        assert!(KCMP_EPOLL_TFD_FD_OFFSET < KCMP_EPOLL_TFD_TOFF_OFFSET);
        assert!(KCMP_EPOLL_TFD_TOFF_OFFSET < KCMP_EPOLL_TFD_TFD_OFFSET);
    }

    #[test]
    fn test_epoll_slot_size() {
        assert_eq!(KCMP_EPOLL_SLOT_SIZE, 12);
    }
}
