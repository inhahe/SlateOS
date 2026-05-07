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
