//! Kernel error types.
//!
//! All kernel subsystems return errors from this unified enum.  Each
//! variant carries enough context for the caller to decide what to do.
//! Error codes are stable across kernel versions (part of the Tier 1
//! stable ABI).
//!
//! Design: no heap allocation in error types.  Errors are small (single
//! discriminant + optional payload) and can be returned by value through
//! registers.

use core::fmt;

/// Top-level kernel error.
///
/// Every fallible kernel function returns `Result<T, KernelError>`.
/// Variants are organized by subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
#[allow(dead_code)] // Variants are part of the stable ABI; all will be used as subsystems mature.
pub enum KernelError {
    // --- General (0 - 99) ---
    /// An operation that should have been valid produced an unexpected state.
    InternalError = -1,
    /// A required feature or resource is not available.
    NotSupported = -2,
    /// An argument to a syscall or internal function was invalid.
    InvalidArgument = -3,
    /// The requested operation would block, but non-blocking was requested.
    WouldBlock = -4,
    /// The operation was cancelled (e.g., handle closed while waiting).
    Cancelled = -5,
    /// A timeout expired before the operation completed.
    TimedOut = -6,
    /// The operation would deadlock (e.g. locking a PI futex the caller
    /// already owns).  Maps to `EDEADLK`.
    Deadlock = -7,
    /// A blocking operation was interrupted by a deliverable signal before it
    /// completed (e.g. a `FUTEX_WAIT` woken by a signal rather than a
    /// `FUTEX_WAKE`).  Maps to `EINTR`; restartable syscalls translate this to
    /// an `ERESTART*` sentinel at the syscall layer instead.
    Interrupted = -8,

    // --- Memory (100 - 199) ---
    /// No physical memory available to satisfy the allocation.
    OutOfMemory = -100,
    /// The virtual address range is invalid or already in use.
    InvalidAddress = -101,
    /// A page fault could not be resolved (e.g., access to unmapped memory).
    PageFault = -102,
    /// Memory alignment requirement not met.
    BadAlignment = -103,

    // --- Process (200 - 299) ---
    /// The referenced process or thread does not exist.
    NoSuchProcess = -200,
    /// The ELF binary is malformed or unsupported.
    InvalidExecutable = -201,
    /// The process has exited.
    ProcessExited = -202,
    /// The calling process has no child processes to wait for (ECHILD).
    NoChildProcess = -203,

    // --- IPC (300 - 399) ---
    /// The channel or pipe has been closed by the other end.
    ChannelClosed = -300,
    /// The send buffer is full and the operation is non-blocking.
    ChannelFull = -301,
    /// The message exceeds the maximum allowed size.
    MessageTooLarge = -302,
    /// A counter or resource count would overflow its maximum.
    Overflow = -303,
    /// A kernel resource limit has been reached (too many objects).
    ResourceExhausted = -304,

    // --- Capability (400 - 499) ---
    /// The caller lacks the required capability for this operation.
    PermissionDenied = -400,
    /// The capability handle is invalid or has been revoked.
    InvalidCapability = -401,

    // --- Filesystem (500 - 599) ---
    /// The file, directory, or path does not exist.
    NotFound = -500,
    /// The target already exists (e.g., creating a file that exists).
    AlreadyExists = -501,
    /// The target is not a directory when a directory was expected.
    NotADirectory = -502,
    /// The target is a directory when a file was expected.
    IsADirectory = -503,
    /// The filesystem or disk is full.
    DiskFull = -504,
    /// The handle refers to a resource that is not of the expected type.
    InvalidHandle = -505,
    /// Too many symbolic links encountered during path resolution.
    TooManyLinks = -506,
    /// The directory is not empty (e.g., rmdir on non-empty directory).
    NotEmpty = -507,
    /// Data integrity check failed (e.g., checksum mismatch).
    CorruptedData = -508,
    /// The filesystem is mounted read-only; write operations are denied.
    ReadOnlyFilesystem = -509,
    /// Too many open file descriptors (EMFILE).
    TooManyOpenFiles = -510,
    /// File size exceeds the allowed limit (EFBIG).
    FileTooLarge = -511,
    /// An operation that requires both operands on the same filesystem was
    /// attempted across a mount boundary (e.g. `RENAME_EXCHANGE` or a hard
    /// link spanning two mounts).  Maps to `EXDEV`.
    CrossDevice = -512,

    // --- Device / I/O (600 - 699) ---
    /// An I/O operation failed at the hardware level.
    IoError = -600,
    /// The referenced device does not exist or is not attached.
    NoSuchDevice = -601,
    /// The device is busy and cannot accept the operation right now.
    DeviceBusy = -602,

    // --- Network (700 - 799) ---
    /// A connection attempt was actively refused by the peer or the netstack
    /// could not establish it (no upstream / RST).  Maps to `ECONNREFUSED`.
    ConnectionRefused = -700,
    /// The socket is not connected and the operation requires an established
    /// connection (e.g. `send`/`recv` on an unconnected stream socket).  Maps
    /// to `ENOTCONN`.
    NotConnected = -701,
    /// A non-blocking `connect` has started but the TCP handshake is not yet
    /// complete.  Maps to `EINPROGRESS`: the caller should `poll`/`epoll` for
    /// `POLLOUT` and then check `getsockopt(SO_ERROR)` for the outcome.
    InProgress = -702,
    /// A `connect` is already in progress on this socket (a repeated non-blocking
    /// `connect` before the first handshake resolved).  Maps to `EALREADY`.
    ConnectAlready = -703,
}

impl KernelError {
    /// Human-readable short description of this error.
    ///
    /// These messages are part of the stable ABI — do not change wording
    /// once a version ships.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::InternalError => "internal kernel error",
            Self::NotSupported => "operation not supported",
            Self::InvalidArgument => "invalid argument",
            Self::WouldBlock => "operation would block",
            Self::Cancelled => "operation cancelled",
            Self::TimedOut => "operation timed out",
            Self::Deadlock => "operation would deadlock",
            Self::Interrupted => "interrupted by signal",
            Self::OutOfMemory => "out of memory",
            Self::InvalidAddress => "invalid address",
            Self::PageFault => "unresolvable page fault",
            Self::BadAlignment => "bad alignment",
            Self::NoSuchProcess => "no such process",
            Self::InvalidExecutable => "invalid executable",
            Self::ProcessExited => "process has exited",
            Self::NoChildProcess => "no child processes",
            Self::ChannelClosed => "channel closed",
            Self::ChannelFull => "channel buffer full",
            Self::MessageTooLarge => "message too large",
            Self::Overflow => "counter overflow",
            Self::ResourceExhausted => "resource limit reached",
            Self::PermissionDenied => "permission denied",
            Self::InvalidCapability => "invalid capability",
            Self::NotFound => "not found",
            Self::AlreadyExists => "already exists",
            Self::NotADirectory => "not a directory",
            Self::IsADirectory => "is a directory",
            Self::DiskFull => "disk full",
            Self::InvalidHandle => "invalid handle",
            Self::TooManyLinks => "too many symbolic links",
            Self::NotEmpty => "directory not empty",
            Self::CorruptedData => "data integrity check failed",
            Self::ReadOnlyFilesystem => "read-only filesystem",
            Self::TooManyOpenFiles => "too many open files",
            Self::FileTooLarge => "file too large",
            Self::CrossDevice => "cross-device operation not permitted",
            Self::IoError => "I/O error",
            Self::NoSuchDevice => "no such device",
            Self::DeviceBusy => "device busy",
            Self::ConnectionRefused => "connection refused",
            Self::NotConnected => "socket not connected",
            Self::InProgress => "operation now in progress",
            Self::ConnectAlready => "connection already in progress",
        }
    }

    /// The stable integer error code.
    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message(), self.code())
    }
}

/// Convenience type alias used throughout the kernel.
pub type KernelResult<T> = Result<T, KernelError>;
