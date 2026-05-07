//! POSIX file control flags and constants.
//!
//! Flags for `open()`, `fcntl()`, and file descriptor operations.
//! Values match Linux x86_64 for binary compatibility.

// ---------------------------------------------------------------------------
// open() flags
// ---------------------------------------------------------------------------

/// Open for reading only.
pub const O_RDONLY: i32 = 0;
/// Open for writing only.
pub const O_WRONLY: i32 = 1;
/// Open for reading and writing.
pub const O_RDWR: i32 = 2;
/// Access mode mask (low 2 bits).
pub const O_ACCMODE: i32 = 3;

/// Create file if it does not exist.
pub const O_CREAT: i32 = 0o100;
/// Error if O_CREAT and file already exists.
pub const O_EXCL: i32 = 0o200;
/// No controlling terminal.
pub const O_NOCTTY: i32 = 0o400;
/// Truncate file to zero length.
pub const O_TRUNC: i32 = 0o1000;
/// Set append mode.
pub const O_APPEND: i32 = 0o2000;
/// Non-blocking mode.
pub const O_NONBLOCK: i32 = 0o4000;
/// Synchronous writes.
pub const O_SYNC: i32 = 0o4_010_000;
/// Don't follow symlinks (on final component).
pub const O_NOFOLLOW: i32 = 0o400_000;
/// Close on exec.
pub const O_CLOEXEC: i32 = 0o2_000_000;
/// Open directory only.
pub const O_DIRECTORY: i32 = 0o200_000;

// ---------------------------------------------------------------------------
// lseek() whence
// ---------------------------------------------------------------------------

/// Seek relative to beginning of file.
pub const SEEK_SET: i32 = 0;
/// Seek relative to current position.
pub const SEEK_CUR: i32 = 1;
/// Seek relative to end of file.
pub const SEEK_END: i32 = 2;

// ---------------------------------------------------------------------------
// access() mode flags
// ---------------------------------------------------------------------------

/// Test for existence.
pub const F_OK: i32 = 0;
/// Test for execute/search permission.
pub const X_OK: i32 = 1;
/// Test for write permission.
pub const W_OK: i32 = 2;
/// Test for read permission.
pub const R_OK: i32 = 4;

// ---------------------------------------------------------------------------
// File type mode bits (for stat)
// ---------------------------------------------------------------------------

/// Bit mask for file type.
pub const S_IFMT: u32 = 0o170_000;
/// Directory.
pub const S_IFDIR: u32 = 0o040_000;
/// Character device.
pub const S_IFCHR: u32 = 0o020_000;
/// Block device.
pub const S_IFBLK: u32 = 0o060_000;
/// Regular file.
pub const S_IFREG: u32 = 0o100_000;
/// FIFO (named pipe).
pub const S_IFIFO: u32 = 0o010_000;
/// Symbolic link.
pub const S_IFLNK: u32 = 0o120_000;
/// Socket.
pub const S_IFSOCK: u32 = 0o140_000;

/// Set user ID on execution.
pub const S_ISUID: u32 = 0o4000;
/// Set group ID on execution.
pub const S_ISGID: u32 = 0o2000;
/// Sticky bit.
pub const S_ISVTX: u32 = 0o1000;

/// Owner read.
pub const S_IRUSR: u32 = 0o400;
/// Owner write.
pub const S_IWUSR: u32 = 0o200;
/// Owner execute.
pub const S_IXUSR: u32 = 0o100;
/// Group read.
pub const S_IRGRP: u32 = 0o040;
/// Group write.
pub const S_IWGRP: u32 = 0o020;
/// Group execute.
pub const S_IXGRP: u32 = 0o010;
/// Others read.
pub const S_IROTH: u32 = 0o004;
/// Others write.
pub const S_IWOTH: u32 = 0o002;
/// Others execute.
pub const S_IXOTH: u32 = 0o001;

/// Default file permissions (owner rw, group/other r).
pub const DEFAULT_FILE_MODE: u32 = S_IRUSR | S_IWUSR | S_IRGRP | S_IROTH;
/// Default directory permissions (owner rwx, group/other rx).
pub const DEFAULT_DIR_MODE: u32 = S_IRUSR | S_IWUSR | S_IXUSR | S_IRGRP | S_IXGRP | S_IROTH | S_IXOTH;
