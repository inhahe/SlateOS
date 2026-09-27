//! The SlateOS kernel's native error codes, and what each one means.
//!
//! A native syscall -- the SlateOS ABI, as opposed to the Linux-compatible
//! entry points, which translate to errno -- fails with the kernel's own
//! code: `KernelError`'s discriminant (`kernel/src/error.rs`), returned
//! unchanged by `SyscallResult::err`. "Permission denied" is `-400`, not
//! Linux's `-1`; `-2` is "operation not supported", not `ENOENT`.
//!
//! A program that decodes those codes with Linux's numbers says the wrong
//! thing, and lane E found five that did (`ifconfig`, `ip`, `route`, `arp`,
//! `dhcpcd`: `requests/e-b-five-network-tools-read-a-refusal-as-error-400.md`)
//! -- their "need root" message could never print, and `arp` named a refusal
//! as a missing file. So the table is here, once, with the kernel's own words:
//! `KernelError::message` is declared stable ABI, and the tests read
//! `kernel/src/error.rs` itself, so an entry here that disagrees with the
//! kernel fails. (A variant the kernel *adds* is tolerated until it is copied
//! here -- a tool says `error N` for it meanwhile -- so that lane A's new
//! error never breaks lane B's tests.)
//!
//! Callers add their own context ("need root") where a code has an obvious
//! cause in their setting, and fall back to [`message`] for the rest.

/// One kernel error: its code, its variant name, and `KernelError::message`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelError {
    /// The code a native syscall returns.
    pub code: i64,
    /// The variant's name in `kernel/src/error.rs`.
    pub name: &'static str,
    /// `KernelError::message`: the kernel's stable wording.
    pub message: &'static str,
}

/// `PermissionDenied`: the caller lacks the capability the operation needs.
pub const PERMISSION_DENIED: i64 = -400;
/// `NotSupported`: a registered handler cannot do what was asked.
pub const NOT_SUPPORTED: i64 = -2;
/// `InvalidArgument`.
pub const INVALID_ARGUMENT: i64 = -3;
/// `NoSuchSyscall`: this kernel has no such entry point at all.
pub const NO_SUCH_SYSCALL: i64 = -10;
/// `NoSuchDevice`.
pub const NO_SUCH_DEVICE: i64 = -601;

const fn e(code: i64, name: &'static str, message: &'static str) -> KernelError {
    KernelError {
        code,
        name,
        message,
    }
}

/// Every `KernelError` variant, in the kernel's order.
pub const ALL: [KernelError; 50] = [
    e(-1, "InternalError", "internal kernel error"),
    e(-2, "NotSupported", "operation not supported"),
    e(-3, "InvalidArgument", "invalid argument"),
    e(-4, "WouldBlock", "operation would block"),
    e(-5, "Cancelled", "operation cancelled"),
    e(-6, "TimedOut", "operation timed out"),
    e(-7, "Deadlock", "operation would deadlock"),
    e(-8, "Interrupted", "interrupted by signal"),
    e(
        -9,
        "BufferTooSmall",
        "output buffer too small for the whole answer",
    ),
    e(
        -10,
        "NoSuchSyscall",
        "no such syscall (no handler registered)",
    ),
    e(-100, "OutOfMemory", "out of memory"),
    e(-101, "InvalidAddress", "invalid address"),
    e(-102, "PageFault", "unresolvable page fault"),
    e(-103, "BadAlignment", "bad alignment"),
    e(-200, "NoSuchProcess", "no such process"),
    e(-201, "InvalidExecutable", "invalid executable"),
    e(-202, "ProcessExited", "process has exited"),
    e(-203, "NoChildProcess", "no child processes"),
    e(-300, "ChannelClosed", "channel closed"),
    e(-301, "ChannelFull", "channel buffer full"),
    e(-302, "MessageTooLarge", "message too large"),
    e(-303, "Overflow", "counter overflow"),
    e(-304, "ResourceExhausted", "resource limit reached"),
    e(-400, "PermissionDenied", "permission denied"),
    e(-401, "InvalidCapability", "invalid capability"),
    e(-500, "NotFound", "not found"),
    e(-501, "AlreadyExists", "already exists"),
    e(-502, "NotADirectory", "not a directory"),
    e(-503, "IsADirectory", "is a directory"),
    e(-504, "DiskFull", "disk full"),
    e(-505, "InvalidHandle", "invalid handle"),
    e(-506, "TooManyLinks", "too many symbolic links"),
    e(-507, "NotEmpty", "directory not empty"),
    e(-508, "CorruptedData", "data integrity check failed"),
    e(-509, "ReadOnlyFilesystem", "read-only filesystem"),
    e(-510, "TooManyOpenFiles", "too many open files"),
    e(-511, "FileTooLarge", "file too large"),
    e(-512, "CrossDevice", "cross-device operation not permitted"),
    e(
        -513,
        "StaleHandle",
        "directory handle no longer denotes the same directory",
    ),
    e(-514, "NoAttribute", "no such extended attribute"),
    e(-600, "IoError", "I/O error"),
    e(-601, "NoSuchDevice", "no such device"),
    e(-602, "DeviceBusy", "device busy"),
    e(-700, "ConnectionRefused", "connection refused"),
    e(-701, "NotConnected", "socket not connected"),
    e(-702, "InProgress", "operation now in progress"),
    e(-703, "ConnectAlready", "connection already in progress"),
    e(-704, "BrokenPipe", "broken pipe (write side shut down)"),
    e(-705, "AddrInUse", "address already in use"),
    e(-706, "MsgSize", "message too long for datagram"),
];

/// The error a code names, if it names one.
#[must_use]
pub fn lookup(code: i64) -> Option<&'static KernelError> {
    ALL.iter().find(|k| k.code == code)
}

/// `KernelError::message` for a code, if it names one.
#[must_use]
pub fn message(code: i64) -> Option<&'static str> {
    lookup(code).map(|k| k.message)
}

/// The kernel's words for a code, or `error CODE` for one it does not
/// define -- what a tool says when it has nothing more specific.
#[must_use]
pub fn describe(code: i64) -> String {
    message(code).map_or_else(|| format!("error {code}"), str::to_string)
}

#[cfg(test)]
mod tests;
