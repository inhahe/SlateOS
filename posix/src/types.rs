//! POSIX type definitions.
//!
//! Provides the standard POSIX type aliases used throughout the
//! compatibility library.  These match the LP64 data model used
//! by our x86_64 target (and Linux x86_64).

/// Process ID.
pub type PidT = i32;

/// User ID.
pub type UidT = u32;

/// Group ID.
pub type GidT = u32;

/// File mode (permissions + type).
pub type ModeT = u32;

/// Device number.
pub type DevT = u64;

/// Inode number.
pub type InoT = u64;

/// Number of hard links.
pub type NlinkT = u64;

/// File offset / size.
pub type OffT = i64;

/// Signed size (return from read/write).
pub type SsizeT = isize;

/// Unsigned size.
pub type SizeT = usize;

/// Block size for I/O.
pub type BlksizeT = i64;

/// Number of 512-byte blocks.
pub type BlkcntT = i64;

/// Time in seconds since epoch.
pub type TimeT = i64;

/// Nanoseconds component of a timespec.
pub type SusecondsT = i64;

/// Clock ID for clock_gettime.
pub type ClockidT = i32;

/// File descriptor.
pub type Fd = i32;

/// Generic ID type (used by waitid, etc.).
pub type IdT = u32;

/// IPC key (System V IPC).
pub type KeyT = i32;

/// Microseconds type (for usleep, etc.).
pub type UsecT = u32;

/// Pointer-sized signed integer.
pub type IntptrT = isize;

/// Pointer-sized unsigned integer.
pub type UintptrT = usize;

/// Pointer difference type.
pub type PtrdiffT = isize;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Type sizes must match LP64 data model (x86_64 Linux) --

    #[test]
    fn test_pid_t_size() {
        assert_eq!(core::mem::size_of::<PidT>(), 4);
    }

    #[test]
    fn test_uid_gid_size() {
        assert_eq!(core::mem::size_of::<UidT>(), 4);
        assert_eq!(core::mem::size_of::<GidT>(), 4);
    }

    #[test]
    fn test_mode_t_size() {
        assert_eq!(core::mem::size_of::<ModeT>(), 4);
    }

    #[test]
    fn test_dev_ino_size() {
        assert_eq!(core::mem::size_of::<DevT>(), 8);
        assert_eq!(core::mem::size_of::<InoT>(), 8);
    }

    #[test]
    fn test_nlink_t_size() {
        assert_eq!(core::mem::size_of::<NlinkT>(), 8);
    }

    #[test]
    fn test_off_t_size() {
        assert_eq!(core::mem::size_of::<OffT>(), 8);
    }

    #[test]
    fn test_ssize_size_t_size() {
        assert_eq!(core::mem::size_of::<SsizeT>(), 8);
        assert_eq!(core::mem::size_of::<SizeT>(), 8);
    }

    #[test]
    fn test_blksize_blkcnt_size() {
        assert_eq!(core::mem::size_of::<BlksizeT>(), 8);
        assert_eq!(core::mem::size_of::<BlkcntT>(), 8);
    }

    #[test]
    fn test_time_t_size() {
        assert_eq!(core::mem::size_of::<TimeT>(), 8);
    }

    #[test]
    fn test_suseconds_t_size() {
        assert_eq!(core::mem::size_of::<SusecondsT>(), 8);
    }

    #[test]
    fn test_clockid_t_size() {
        assert_eq!(core::mem::size_of::<ClockidT>(), 4);
    }

    #[test]
    fn test_fd_size() {
        assert_eq!(core::mem::size_of::<Fd>(), 4);
    }

    #[test]
    fn test_id_t_size() {
        assert_eq!(core::mem::size_of::<IdT>(), 4);
    }

    #[test]
    fn test_key_t_size() {
        assert_eq!(core::mem::size_of::<KeyT>(), 4);
    }

    #[test]
    fn test_pointer_types_size() {
        assert_eq!(core::mem::size_of::<IntptrT>(), 8);
        assert_eq!(core::mem::size_of::<UintptrT>(), 8);
        assert_eq!(core::mem::size_of::<PtrdiffT>(), 8);
    }

    // -- Signedness checks --

    #[test]
    fn test_pid_t_signed() {
        let neg: PidT = -1;
        assert!(neg < 0);
    }

    #[test]
    fn test_off_t_signed() {
        let neg: OffT = -1;
        assert!(neg < 0);
    }

    #[test]
    fn test_ssize_t_signed() {
        let neg: SsizeT = -1;
        assert!(neg < 0);
    }

    #[test]
    fn test_time_t_signed() {
        let neg: TimeT = -1;
        assert!(neg < 0);
    }

    #[test]
    fn test_uid_gid_unsigned() {
        let u: UidT = u32::MAX;
        let g: GidT = u32::MAX;
        assert!(u > 0);
        assert!(g > 0);
    }

    // -- Alignment tests --

    #[test]
    fn test_pid_t_alignment() {
        assert_eq!(core::mem::align_of::<PidT>(), 4);
    }

    #[test]
    fn test_off_t_alignment() {
        assert_eq!(core::mem::align_of::<OffT>(), 8);
    }

    #[test]
    fn test_dev_t_alignment() {
        assert_eq!(core::mem::align_of::<DevT>(), 8);
    }

    #[test]
    fn test_size_t_alignment() {
        assert_eq!(core::mem::align_of::<SizeT>(), 8);
        assert_eq!(core::mem::align_of::<SsizeT>(), 8);
    }

    #[test]
    fn test_pointer_types_alignment() {
        // Must match pointer alignment for FFI.
        assert_eq!(
            core::mem::align_of::<IntptrT>(),
            core::mem::align_of::<*const u8>()
        );
        assert_eq!(
            core::mem::align_of::<UintptrT>(),
            core::mem::align_of::<*const u8>()
        );
    }

    // -- Relationship tests --

    #[test]
    fn test_ssize_t_is_signed_size_t() {
        // SsizeT should have the same width as SizeT but be signed.
        assert_eq!(
            core::mem::size_of::<SsizeT>(),
            core::mem::size_of::<SizeT>()
        );
        let neg: SsizeT = -1;
        assert!(neg < 0, "SsizeT must be signed");
    }

    #[test]
    fn test_intptr_uintptr_same_size() {
        assert_eq!(
            core::mem::size_of::<IntptrT>(),
            core::mem::size_of::<UintptrT>()
        );
    }

    #[test]
    fn test_intptr_holds_pointer() {
        // IntptrT must be large enough to hold a pointer.
        assert_eq!(
            core::mem::size_of::<IntptrT>(),
            core::mem::size_of::<*const u8>()
        );
    }

    #[test]
    fn test_ptrdiff_t_is_signed() {
        let neg: PtrdiffT = -100;
        assert!(neg < 0);
    }

    // -- Additional signedness checks --

    #[test]
    fn test_blksize_t_signed() {
        let neg: BlksizeT = -1;
        assert!(neg < 0);
    }

    #[test]
    fn test_blkcnt_t_signed() {
        let neg: BlkcntT = -1;
        assert!(neg < 0);
    }

    #[test]
    fn test_suseconds_t_signed() {
        let neg: SusecondsT = -1;
        assert!(neg < 0);
    }

    #[test]
    fn test_key_t_signed() {
        let neg: KeyT = -1;
        assert!(neg < 0);
    }

    #[test]
    fn test_clockid_t_signed() {
        let neg: ClockidT = -1;
        assert!(neg < 0);
    }

    #[test]
    fn test_fd_signed() {
        let neg: Fd = -1;
        assert!(
            neg < 0,
            "fd -1 must be representable (common error sentinel)"
        );
    }

    // -- Unsigned type max values --

    #[test]
    fn test_mode_t_unsigned() {
        let max: ModeT = u32::MAX;
        assert!(max > 0);
    }

    #[test]
    fn test_usec_t_unsigned() {
        let max: UsecT = u32::MAX;
        assert!(max > 0);
    }

    #[test]
    fn test_id_t_unsigned() {
        let max: IdT = u32::MAX;
        assert!(max > 0);
    }

    #[test]
    fn test_nlink_t_unsigned() {
        let max: NlinkT = u64::MAX;
        assert!(max > 0);
    }

    #[test]
    fn test_usec_t_size() {
        assert_eq!(core::mem::size_of::<UsecT>(), 4);
    }
}
