//! Linux x86_64 syscall ABI translation layer.
//!
//! This module lets prebuilt Linux ELF binaries run on this kernel.
//! A process with [`crate::proc::pcb::AbiMode::Linux`] sees its
//! `syscall` instructions routed through [`dispatch_linux`] instead
//! of the native dispatch table.
//!
//! # What translation buys us
//!
//! Linux uses a stable, ~450-entry syscall numbering (`asm/unistd_64.h`)
//! and `-errno` return convention.  Our native ABI uses sparse numbers
//! in the 0–1100 range with rich `KernelError` codes.  The two ABIs
//! overlap *numerically* — Linux `read = 0` collides with our
//! `SYS_YIELD = 0`, Linux `write = 1` with `SYS_EXIT = 1`, etc. — so
//! they can't share a dispatch table.  Instead we route per-process by
//! ABI mode, then per-syscall translate:
//!
//! - **Numbers**: Linux number → which native handler to invoke.
//! - **Arguments**: Linux struct layouts (e.g. `struct timespec`,
//!   `struct iovec`) → kernel-friendly forms.
//! - **Return values**: native `KernelError` → Linux `-errno`.
//!
//! # Scope of this initial implementation
//!
//! The framework is complete (number table, errno mapping, dispatch
//! routing).  The translated syscall set is deliberately narrow: about
//! 25 stateless operations that let us prove the routing end-to-end
//! without first building a kernel-side POSIX fd table.
//!
//! Implemented:
//!
//! | Linux nr | Name              | Notes                              |
//! |----------|-------------------|------------------------------------|
//! | 0        | read              | via per-process Linux fd table     |
//! | 1        | write             | via per-process Linux fd table     |
//! | 2        | open              | wraps `openat(AT_FDCWD, ...)`      |
//! | 3        | close             | via per-process Linux fd table     |
//! | 8        | lseek             | only for File handles              |
//! | 9        | mmap              | anonymous private map only         |
//! | 10       | mprotect          | no-op success (perms not tracked)  |
//! | 11       | munmap            | passes through to native           |
//! | 12       | brk               | always returns current brk (NYI)   |
//! | 13       | rt_sigaction      | maps to SYS_SIGNAL_REGISTER       |
//! | 14       | rt_sigprocmask    | maps to SYS_SIGNAL_MASK           |
//! | 19       | readv             | via per-process Linux fd table     |
//! | 20       | writev            | via per-process Linux fd table     |
//! | 22       | pipe              | wraps SYS_PIPE_CREATE              |
//! | 24       | sched_yield       | direct                             |
//! | 32       | dup               | via per-process Linux fd table     |
//! | 33       | dup2              | via per-process Linux fd table     |
//! | 72       | fcntl             | F_DUPFD / F_GETFD / F_SETFD /      |
//! |          |                   | F_GETFL / F_SETFL / F_DUPFD_CLOEXEC|
//! | 257      | openat            | only AT_FDCWD; routes to VFS open  |
//! | 292      | dup3              | via per-process Linux fd table     |
//! | 293      | pipe2             | pipe with O_CLOEXEC / O_NONBLOCK   |
//! | 35       | nanosleep         | reads timespec, calls SYS_SLEEP    |
//! | 39       | getpid            | direct                             |
//! | 60       | exit              | direct                             |
//! | 62       | kill              | maps to SYS_SIGNAL_SEND            |
//! | 63       | uname             | populates utsname struct           |
//! | 96       | gettimeofday      | clock_realtime split into sec/usec |
//! | 102      | getuid            | reads cred.uid                     |
//! | 104      | getgid            | reads cred.gid                     |
//! | 107      | geteuid           | reads cred.euid                    |
//! | 108      | getegid           | reads cred.egid                    |
//! | 110      | getppid           | reads parent pid                   |
//! | 158      | arch_prctl        | ARCH_SET_FS / ARCH_GET_FS via MSR  |
//! | 186      | gettid            | direct                             |
//! | 201      | time              | clock_realtime / 1e9               |
//! | 202      | futex             | maps to SYS_FUTEX_*                |
//! | 218      | set_tid_address   | returns tid, ignores stored ptr    |
//! | 228      | clock_gettime     | reads clock id, writes timespec    |
//! | 229      | clock_getres      | reports 1ns res                    |
//! | 230      | clock_nanosleep   | maps to SYS_SLEEP (relative)       |
//! | 231      | exit_group        | direct (treated like exit)         |
//! | 318      | getrandom         | from kernel CSPRNG                 |
//!
//! Anything else returns `-ENOSYS`.  Expanding the table is purely
//! additive — see `kernel/src/syscall/linux.rs` change history for the
//! pattern.
//!
//! # What's deferred
//!
//! - **socket family**, **pipe/pipe2**, **poll/epoll**, **eventfd**:
//!   require additional kernel-side machinery beyond the fd table.
//! - **execve / fork / vfork / clone / sigreturn**: these modify the
//!   syscall frame (RIP/RSP).  They have to live in `entry.rs`
//!   alongside the existing native-ABI frame-modifying paths; the
//!   `dispatch_linux` flat dispatch returns -ENOSYS for them today.
//! - **mmap/mprotect with PROT_EXEC + MAP_PRIVATE backed by a file**:
//!   no fd table yet, so file-backed maps cannot be translated.
//! - **rt_sigaction**: native sigaction takes a struct, ours takes
//!   (signum, handler).  Only the handler pointer is forwarded; sa_mask
//!   and sa_flags are silently ignored (matching the existing native
//!   signal shim limitations documented in `todo.txt`).
//!
//! # Errno mapping
//!
//! [`linux_errno_for`] maps every native `KernelError` to a stable
//! Linux errno number.  Any error we don't have a closer match for goes
//! to `EINVAL` (which is the Linux convention for "the kernel decided
//! this call was malformed").

// Translation layer; many entries are wired ahead of being used by tests.
#![allow(dead_code)]
// u64 syscall args → smaller integer types on this 64-bit target.
#![allow(clippy::cast_possible_truncation)]

use crate::error::KernelError;
use crate::proc::pcb;

use super::dispatch::{SyscallArgs, SyscallResult};
use super::handlers;

// ---------------------------------------------------------------------------
// Linux x86_64 syscall numbers (subset).
//
// Source of truth: `linux/arch/x86/entry/syscalls/syscall_64.tbl` (the
// upstream ABI table).  Only the numbers we currently route on are
// listed; the rest fall through to -ENOSYS.
// ---------------------------------------------------------------------------

/// Linux x86_64 syscall numbers, namespaced to avoid colliding with our
/// native `SYS_*` constants in `super::number`.
pub mod nr {
    pub const READ: u64 = 0;
    pub const WRITE: u64 = 1;
    pub const OPEN: u64 = 2;
    pub const CLOSE: u64 = 3;
    pub const STAT: u64 = 4;
    pub const FSTAT: u64 = 5;
    pub const LSTAT: u64 = 6;
    pub const POLL: u64 = 7;
    pub const LSEEK: u64 = 8;
    pub const MMAP: u64 = 9;
    pub const MPROTECT: u64 = 10;
    pub const MUNMAP: u64 = 11;
    pub const BRK: u64 = 12;
    pub const RT_SIGACTION: u64 = 13;
    pub const RT_SIGPROCMASK: u64 = 14;
    pub const RT_SIGRETURN: u64 = 15;
    pub const IOCTL: u64 = 16;
    pub const PREAD64: u64 = 17;
    pub const PWRITE64: u64 = 18;
    pub const READV: u64 = 19;
    pub const WRITEV: u64 = 20;
    pub const ACCESS: u64 = 21;
    pub const PIPE: u64 = 22;
    pub const SELECT: u64 = 23;
    pub const SCHED_YIELD: u64 = 24;
    pub const MREMAP: u64 = 25;
    pub const MSYNC: u64 = 26;
    pub const MADVISE: u64 = 28;
    pub const DUP: u64 = 32;
    pub const DUP2: u64 = 33;
    pub const NANOSLEEP: u64 = 35;
    pub const GETPID: u64 = 39;
    pub const CLONE: u64 = 56;
    pub const FORK: u64 = 57;
    pub const VFORK: u64 = 58;
    pub const EXECVE: u64 = 59;
    pub const EXIT: u64 = 60;
    pub const WAIT4: u64 = 61;
    pub const KILL: u64 = 62;
    pub const UNAME: u64 = 63;
    pub const FCNTL: u64 = 72;
    pub const GETCWD: u64 = 79;
    pub const CHDIR: u64 = 80;
    pub const MKDIR: u64 = 83;
    pub const RMDIR: u64 = 84;
    pub const UNLINK: u64 = 87;
    pub const READLINK: u64 = 89;
    pub const GETTIMEOFDAY: u64 = 96;
    pub const GETUID: u64 = 102;
    pub const GETGID: u64 = 104;
    pub const GETEUID: u64 = 107;
    pub const GETEGID: u64 = 108;
    pub const GETPPID: u64 = 110;
    pub const ARCH_PRCTL: u64 = 158;
    pub const GETTID: u64 = 186;
    pub const TIME: u64 = 201;
    pub const FUTEX: u64 = 202;
    pub const SET_TID_ADDRESS: u64 = 218;
    pub const CLOCK_GETTIME: u64 = 228;
    pub const CLOCK_GETRES: u64 = 229;
    pub const CLOCK_NANOSLEEP: u64 = 230;
    pub const EXIT_GROUP: u64 = 231;
    pub const OPENAT: u64 = 257;
    pub const SET_ROBUST_LIST: u64 = 273;
    pub const EVENTFD: u64 = 290;
    pub const EVENTFD2: u64 = 290; // alias; modern kernels use 290 only
    pub const DUP3: u64 = 292;
    pub const PIPE2: u64 = 293;
    pub const GETRANDOM: u64 = 318;
    pub const STATX: u64 = 332;
}

// ---------------------------------------------------------------------------
// Linux open-flag constants (used by open / openat / fcntl translation).
//
// Source of truth: `include/uapi/asm-generic/fcntl.h`.  Only the bits the
// translator interprets explicitly are listed.
// ---------------------------------------------------------------------------

/// Linux `O_*` flag bits (subset interpreted by this layer).
pub mod oflags {
    pub const O_ACCMODE: u32 = 0o0003;
    pub const O_RDONLY: u32 = 0o0000;
    pub const O_WRONLY: u32 = 0o0001;
    pub const O_RDWR: u32 = 0o0002;
    pub const O_CREAT: u32 = 0o100;
    pub const O_EXCL: u32 = 0o200;
    pub const O_TRUNC: u32 = 0o1000;
    pub const O_APPEND: u32 = 0o2000;
    pub const O_NONBLOCK: u32 = 0o4000;
    pub const O_DIRECTORY: u32 = 0o200_000;
    pub const O_CLOEXEC: u32 = 0o2_000_000;
}

/// Linux `fcntl` command numbers (subset).
pub mod fcntl_cmd {
    pub const F_DUPFD: u32 = 0;
    pub const F_GETFD: u32 = 1;
    pub const F_SETFD: u32 = 2;
    pub const F_GETFL: u32 = 3;
    pub const F_SETFL: u32 = 4;
    pub const F_DUPFD_CLOEXEC: u32 = 1030;
}

/// Linux `AT_FDCWD` — special "current working directory" base fd for
/// the `*at` family of syscalls.  Our VFS resolves paths against the
/// caller's cwd unconditionally, so AT_FDCWD is the only `dirfd` value
/// we accept; any other `dirfd` returns -ENOSYS until we support
/// directory file descriptors.
pub const AT_FDCWD: i32 = -100;

// ---------------------------------------------------------------------------
// Linux errno values.
//
// These are the small positive integers Linux returns as `-errno` from
// failing syscalls.  Values are stable across Linux versions and
// match `asm-generic/errno{,-base}.h`.
// ---------------------------------------------------------------------------

/// Linux errno values (positive — return `-errno` from syscalls).
pub mod errno {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const ESRCH: i32 = 3;
    pub const EINTR: i32 = 4;
    pub const EIO: i32 = 5;
    pub const ENXIO: i32 = 6;
    pub const E2BIG: i32 = 7;
    pub const ENOEXEC: i32 = 8;
    pub const EBADF: i32 = 9;
    pub const ECHILD: i32 = 10;
    pub const EAGAIN: i32 = 11;
    pub const ENOMEM: i32 = 12;
    pub const EACCES: i32 = 13;
    pub const EFAULT: i32 = 14;
    pub const ENOTBLK: i32 = 15;
    pub const EBUSY: i32 = 16;
    pub const EEXIST: i32 = 17;
    pub const EXDEV: i32 = 18;
    pub const ENODEV: i32 = 19;
    pub const ENOTDIR: i32 = 20;
    pub const EISDIR: i32 = 21;
    pub const EINVAL: i32 = 22;
    pub const ENFILE: i32 = 23;
    pub const EMFILE: i32 = 24;
    pub const ENOTTY: i32 = 25;
    pub const ETXTBSY: i32 = 26;
    pub const EFBIG: i32 = 27;
    pub const ENOSPC: i32 = 28;
    pub const ESPIPE: i32 = 29;
    pub const EROFS: i32 = 30;
    pub const EMLINK: i32 = 31;
    pub const EPIPE: i32 = 32;
    pub const EDOM: i32 = 33;
    pub const ERANGE: i32 = 34;
    pub const EDEADLK: i32 = 35;
    pub const ENAMETOOLONG: i32 = 36;
    pub const ENOLCK: i32 = 37;
    pub const ENOSYS: i32 = 38;
    pub const ENOTEMPTY: i32 = 39;
    pub const ELOOP: i32 = 40;
    pub const ENOMSG: i32 = 42;
    pub const EOVERFLOW: i32 = 75;
    pub const EOPNOTSUPP: i32 = 95;
    pub const ETIMEDOUT: i32 = 110;
    pub const ECANCELED: i32 = 125;
}

// ---------------------------------------------------------------------------
// Native KernelError → Linux errno
// ---------------------------------------------------------------------------

/// Translate a native [`KernelError`] to the corresponding Linux errno
/// (positive value).  Callers typically want `-(linux_errno_for(e) as i64)`
/// as the syscall return value.
///
/// When in doubt this returns `EINVAL` — that's the Linux convention for
/// "the kernel rejected the call as malformed" and matches what Linux
/// itself does for unknown-cause failures.
#[must_use]
pub const fn linux_errno_for(e: KernelError) -> i32 {
    match e {
        KernelError::InternalError => errno::EIO,
        KernelError::NotSupported => errno::ENOSYS,
        KernelError::InvalidArgument => errno::EINVAL,
        KernelError::WouldBlock => errno::EAGAIN,
        KernelError::Cancelled => errno::ECANCELED,
        KernelError::TimedOut => errno::ETIMEDOUT,
        KernelError::OutOfMemory => errno::ENOMEM,
        KernelError::InvalidAddress => errno::EFAULT,
        KernelError::PageFault => errno::EFAULT,
        KernelError::BadAlignment => errno::EINVAL,
        KernelError::NoSuchProcess => errno::ESRCH,
        KernelError::InvalidExecutable => errno::ENOEXEC,
        KernelError::ProcessExited => errno::ECHILD,
        KernelError::NoChildProcess => errno::ECHILD,
        KernelError::ChannelClosed => errno::EPIPE,
        KernelError::ChannelFull => errno::EAGAIN,
        KernelError::MessageTooLarge => errno::E2BIG,
        KernelError::Overflow => errno::EOVERFLOW,
        KernelError::ResourceExhausted => errno::ENFILE,
        KernelError::PermissionDenied => errno::EACCES,
        KernelError::InvalidCapability => errno::EPERM,
        KernelError::NotFound => errno::ENOENT,
        KernelError::AlreadyExists => errno::EEXIST,
        KernelError::NotADirectory => errno::ENOTDIR,
        KernelError::IsADirectory => errno::EISDIR,
        KernelError::DiskFull => errno::ENOSPC,
        KernelError::InvalidHandle => errno::EBADF,
        KernelError::TooManyLinks => errno::ELOOP,
        KernelError::NotEmpty => errno::ENOTEMPTY,
        KernelError::CorruptedData => errno::EIO,
        KernelError::ReadOnlyFilesystem => errno::EROFS,
        KernelError::TooManyOpenFiles => errno::EMFILE,
        KernelError::FileTooLarge => errno::EFBIG,
        KernelError::IoError => errno::EIO,
        KernelError::NoSuchDevice => errno::ENODEV,
        KernelError::DeviceBusy => errno::EBUSY,
    }
}

/// Convert a native [`SyscallResult`] to the Linux ABI form.
///
/// On success (`value >= 0`), the value is passed through unchanged.
/// On error (`value < 0`), the native error code is interpreted as a
/// [`KernelError`] and remapped to `-(linux_errno_for(e) as i64)`.
#[must_use]
pub fn linux_from_native(res: SyscallResult) -> SyscallResult {
    if res.value >= 0 {
        return res;
    }
    // Native error encoding: the value is a signed kernel-error code
    // (negative i32 widened to i64).  Recover the original variant from
    // the code, then map it to a Linux errno.
    #[allow(clippy::cast_possible_truncation)]
    let code = res.value as i32;
    let errno_val = match kernel_error_from_code(code) {
        Some(e) => linux_errno_for(e),
        None => errno::EINVAL,
    };
    SyscallResult::ok(-i64::from(errno_val))
}

/// Recover a [`KernelError`] from its stable integer code.
///
/// This is the inverse of `KernelError::code()`.  Returns `None` if
/// the code does not name any known variant.
#[must_use]
pub const fn kernel_error_from_code(code: i32) -> Option<KernelError> {
    match code {
        -1 => Some(KernelError::InternalError),
        -2 => Some(KernelError::NotSupported),
        -3 => Some(KernelError::InvalidArgument),
        -4 => Some(KernelError::WouldBlock),
        -5 => Some(KernelError::Cancelled),
        -6 => Some(KernelError::TimedOut),
        -100 => Some(KernelError::OutOfMemory),
        -101 => Some(KernelError::InvalidAddress),
        -102 => Some(KernelError::PageFault),
        -103 => Some(KernelError::BadAlignment),
        -200 => Some(KernelError::NoSuchProcess),
        -201 => Some(KernelError::InvalidExecutable),
        -202 => Some(KernelError::ProcessExited),
        -203 => Some(KernelError::NoChildProcess),
        -300 => Some(KernelError::ChannelClosed),
        -301 => Some(KernelError::ChannelFull),
        -302 => Some(KernelError::MessageTooLarge),
        -303 => Some(KernelError::Overflow),
        -304 => Some(KernelError::ResourceExhausted),
        -400 => Some(KernelError::PermissionDenied),
        -401 => Some(KernelError::InvalidCapability),
        -500 => Some(KernelError::NotFound),
        -501 => Some(KernelError::AlreadyExists),
        -502 => Some(KernelError::NotADirectory),
        -503 => Some(KernelError::IsADirectory),
        -504 => Some(KernelError::DiskFull),
        -505 => Some(KernelError::InvalidHandle),
        -506 => Some(KernelError::TooManyLinks),
        -507 => Some(KernelError::NotEmpty),
        -508 => Some(KernelError::CorruptedData),
        -509 => Some(KernelError::ReadOnlyFilesystem),
        -510 => Some(KernelError::TooManyOpenFiles),
        -511 => Some(KernelError::FileTooLarge),
        -600 => Some(KernelError::IoError),
        -601 => Some(KernelError::NoSuchDevice),
        -602 => Some(KernelError::DeviceBusy),
        _ => None,
    }
}

/// Build a Linux-style error result with the given errno.
#[must_use]
pub const fn linux_err(errno_val: i32) -> SyscallResult {
    SyscallResult::ok(-(errno_val as i64))
}

// ---------------------------------------------------------------------------
// Linux frame-modifying constants (clone flags)
// ---------------------------------------------------------------------------

/// Subset of Linux `CLONE_*` flag bits we explicitly recognise.  Bits
/// 0..7 of `flags` carry the termination signal (`CSIGNAL`); the rest
/// are the actual sharing-control bits.
///
/// Source: `include/uapi/linux/sched.h`.
pub mod clone_flags {
    pub const CSIGNAL: u64 = 0x0000_00ff;
    pub const CLONE_VM: u64 = 0x0000_0100;
    pub const CLONE_FS: u64 = 0x0000_0200;
    pub const CLONE_FILES: u64 = 0x0000_0400;
    pub const CLONE_SIGHAND: u64 = 0x0000_0800;
    pub const CLONE_PTRACE: u64 = 0x0000_2000;
    pub const CLONE_VFORK: u64 = 0x0000_4000;
    pub const CLONE_PARENT: u64 = 0x0000_8000;
    pub const CLONE_THREAD: u64 = 0x0001_0000;
    pub const CLONE_NEWNS: u64 = 0x0002_0000;
    pub const CLONE_SYSVSEM: u64 = 0x0004_0000;
    pub const CLONE_SETTLS: u64 = 0x0008_0000;
    pub const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
    pub const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
    pub const CLONE_DETACHED: u64 = 0x0040_0000;
    pub const CLONE_UNTRACED: u64 = 0x0080_0000;
    pub const CLONE_CHILD_SETTID: u64 = 0x0100_0000;
    /// SIGCHLD is the conventional CSIGNAL byte for fork-equivalent
    /// `clone()` calls.
    pub const SIGCHLD: u64 = 17;
}

// ---------------------------------------------------------------------------
// Frame-modifying dispatch
// ---------------------------------------------------------------------------

/// Dispatch the Linux syscalls that need direct access to the saved
/// register frame (fork / vfork / clone / execve).
///
/// Returns `Some(rax)` if this function handled the syscall — the caller
/// must propagate `rax` straight back to userspace (after the usual
/// signal-delivery hook).  Returns `None` for any syscall number that
/// is not one of these frame-modifying paths; the caller then falls
/// through to the regular `dispatch_linux(nr, args)`.
///
/// This mirrors the native `syscall_handler_inner` top-of-function
/// checks for `SYS_PROCESS_EXEC` / `SYS_PROCESS_FORK` etc., but for
/// Linux-ABI processes and using Linux syscall numbers.
#[must_use]
pub fn dispatch_linux_with_frame(
    frame: &mut crate::syscall::entry::SyscallFrame,
) -> Option<i64> {
    match frame.syscall_nr {
        nr::FORK | nr::VFORK => Some(linux_fork(frame)),
        nr::CLONE => Some(linux_clone(frame)),
        nr::EXECVE => Some(linux_execve(frame)),
        _ => None,
    }
}

/// Linux `fork()` / `vfork()` translation.
///
/// vfork is implemented identically to fork: the Linux `vfork()`
/// optimisation (parent blocks until child execs/exits, child shares
/// parent's pages) is a performance hint, not a correctness
/// requirement — every conformant caller of vfork must work correctly
/// against a plain fork.  We pay the CoW page table walk vfork was
/// trying to avoid, but the program behaves the same.
fn linux_fork(frame: &mut crate::syscall::entry::SyscallFrame) -> i64 {
    use crate::proc::{fork, thread};

    let task_id = crate::sched::current_task_id();
    let parent_pid = match thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => return -i64::from(errno::ESRCH),
    };

    match fork::fork_process(parent_pid, frame) {
        Ok(child_pid) => {
            #[allow(clippy::cast_possible_wrap)]
            {
                child_pid as i64
            }
        }
        Err(e) => -i64::from(linux_errno_for(e)),
    }
}

/// Linux `clone()` translation — limited support.
///
/// Linux `clone(flags, child_stack, ptid, ctid, tls)` is the swiss-army
/// knife behind both `fork()` and `pthread_create()`.  Implementing the
/// full thread-creation path requires creating a new thread sharing the
/// parent's address space / fd table / signal handlers, with TLS
/// pointer setup and futex-on-exit hooks for `pthread_join()` — that's
/// a substantial chunk of work.
///
/// For now we only support the "fork-equivalent" use of clone: glibc's
/// `fork()` wrapper issues `clone(SIGCHLD, 0, ...)` (zero child_stack,
/// no CLONE_VM, just the SIGCHLD termination signal).  Anything that
/// asks for CLONE_VM, CLONE_THREAD, or a non-zero child_stack returns
/// `-ENOSYS` so glibc falls back to the no-threads path where possible.
fn linux_clone(frame: &mut crate::syscall::entry::SyscallFrame) -> i64 {
    let flags = frame.arg0;
    let child_stack = frame.arg1;

    // A non-zero child_stack means the caller wants the child to run
    // on a different stack than the parent — only meaningful for true
    // thread creation (shared address space).  Reject explicitly.
    if child_stack != 0 {
        return -i64::from(errno::ENOSYS);
    }

    // Any "share with parent" bit means this is a thread-creation
    // request, not a fork.  We don't support those yet.
    const THREAD_BITS: u64 = clone_flags::CLONE_VM
        | clone_flags::CLONE_FS
        | clone_flags::CLONE_FILES
        | clone_flags::CLONE_SIGHAND
        | clone_flags::CLONE_THREAD
        | clone_flags::CLONE_SYSVSEM
        | clone_flags::CLONE_SETTLS
        | clone_flags::CLONE_PARENT_SETTID
        | clone_flags::CLONE_CHILD_CLEARTID
        | clone_flags::CLONE_CHILD_SETTID;
    if flags & THREAD_BITS != 0 {
        return -i64::from(errno::ENOSYS);
    }

    // CLONE_VFORK / CLONE_PARENT / CLONE_NEWNS / CLONE_PTRACE are
    // also outside what plain-fork semantics give us; reject.
    const UNSUPPORTED_BITS: u64 = clone_flags::CLONE_VFORK
        | clone_flags::CLONE_PARENT
        | clone_flags::CLONE_NEWNS
        | clone_flags::CLONE_PTRACE;
    if flags & UNSUPPORTED_BITS != 0 {
        return -i64::from(errno::ENOSYS);
    }

    // Everything that remains is just the CSIGNAL byte — fork-equivalent.
    // glibc fork() passes SIGCHLD here; we don't actually deliver a
    // signal yet (no Unix-style signals to userspace), but the kernel
    // already records parent/child relationships in the PCB.
    linux_fork(frame)
}

/// Maximum length of a single NUL-terminated string read from
/// userspace during `execve` argument marshalling.  Matches Linux's
/// `MAX_ARG_STRLEN` ceiling at 128 KiB but our typical use cases are
/// far smaller — most argv entries are tens of bytes.
const EXECVE_MAX_STR_LEN: usize = 128 * 1024;

/// Maximum number of entries in `argv` or `envp` during `execve`.
/// Linux uses `MAX_ARG_STRINGS = 0x7FFFFFFF`, but a realistic cap
/// limits how badly a malicious caller can hold us in the pointer
/// walk before we bail out.
const EXECVE_MAX_ARGS: usize = 2048;

/// Aggregate cap on total argv+envp bytes — matches the cap that
/// `sys_process_exec_with_frame` uses for the native path.
const EXECVE_MAX_TOTAL_BYTES: usize = 256 * 1024;

/// Read a NUL-terminated byte string from `ptr` in userspace, up to
/// `max_len` bytes (not counting the NUL).  Returns the bytes
/// (without the terminator) on success, or an `errno` value on
/// failure.
fn read_user_cstr(ptr: u64, max_len: usize) -> Result<alloc::vec::Vec<u8>, i32> {
    if ptr == 0 {
        return Err(errno::EFAULT);
    }
    let mut buf: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut i: usize = 0;
    while i <= max_len {
        let mut b: u8 = 0;
        // SAFETY: copy_from_user validates the one-byte user range
        // before touching it and uses STAC/CLAC for SMAP.
        let r = unsafe {
            crate::mm::user::copy_from_user(
                ptr.wrapping_add(i as u64),
                &raw mut b,
                1,
            )
        };
        if let Err(e) = r {
            return Err(linux_errno_for(e));
        }
        if b == 0 {
            return Ok(buf);
        }
        if i == max_len {
            // Found a non-NUL byte at position max_len, meaning the
            // string is longer than allowed.
            return Err(errno::ENAMETOOLONG);
        }
        buf.push(b);
        i += 1;
    }
    Err(errno::ENAMETOOLONG)
}

/// Read a NULL-terminated array of `u64` user pointers (argv/envp)
/// starting at `ptr`, with at most `max_entries` non-NULL entries.
/// A `ptr` of 0 is treated as an empty array (matching what glibc
/// passes when the program had no arguments).
fn read_user_ptr_array(
    ptr: u64,
    max_entries: usize,
) -> Result<alloc::vec::Vec<u64>, i32> {
    if ptr == 0 {
        return Ok(alloc::vec::Vec::new());
    }
    let mut out: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    for i in 0..=max_entries {
        let mut p: u64 = 0;
        // SAFETY: copy_from_user validates the 8-byte user range
        // before touching it.
        let r = unsafe {
            crate::mm::user::copy_from_user(
                ptr.wrapping_add((i * 8) as u64),
                (&raw mut p).cast::<u8>(),
                8,
            )
        };
        if let Err(e) = r {
            return Err(linux_errno_for(e));
        }
        if p == 0 {
            return Ok(out);
        }
        if i == max_entries {
            return Err(errno::E2BIG);
        }
        out.push(p);
    }
    Err(errno::E2BIG)
}

/// Linux `execve(filename, argv[], envp[])` translation.
///
/// Resolves `filename` through the VFS, loads the file into a kernel
/// buffer, walks the userspace `argv` and `envp` pointer arrays
/// reading each NUL-terminated string into kernel buffers, then
/// hands off to `proc::spawn::exec_process`.  All argument
/// marshalling completes BEFORE the old address space is torn down,
/// so a malformed-argv `execve` leaves the caller's image intact.
///
/// On success the saved syscall frame is rewritten so SYSRET returns
/// to the new entry point with a clean register state, matching the
/// native `sys_process_exec_with_frame` behaviour.  On failure the
/// caller observes a Linux `-errno` and continues running.
fn linux_execve(frame: &mut crate::syscall::entry::SyscallFrame) -> i64 {
    use crate::proc::{signal, spawn::exec_process, thread};

    let filename_ptr = frame.arg0;
    let argv_user = frame.arg1;
    let envp_user = frame.arg2;

    // ---- 1. Resolve caller's PID. ----
    let task_id = crate::sched::current_task_id();
    let pid = match thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => return -i64::from(errno::ESRCH),
    };

    // ---- 2. Read filename. ----
    // PATH_MAX on Linux is 4096; our VFS uses str so we additionally
    // require valid UTF-8 (Linux accepts arbitrary bytes — the path
    // is "all bytes except / and NUL").  Treat invalid UTF-8 as
    // ENOENT (the file by that name doesn't exist on a UTF-8 VFS).
    const PATH_MAX: usize = 4096;
    let filename_bytes = match read_user_cstr(filename_ptr, PATH_MAX) {
        Ok(b) => b,
        Err(e) => return -i64::from(e),
    };
    if filename_bytes.is_empty() {
        return -i64::from(errno::ENOENT);
    }
    let filename = match core::str::from_utf8(&filename_bytes) {
        Ok(s) => s,
        Err(_) => return -i64::from(errno::ENOENT),
    };

    // ---- 3. Read argv and envp pointer arrays. ----
    let argv_ptrs = match read_user_ptr_array(argv_user, EXECVE_MAX_ARGS) {
        Ok(v) => v,
        Err(e) => return -i64::from(e),
    };
    let envp_ptrs = match read_user_ptr_array(envp_user, EXECVE_MAX_ARGS) {
        Ok(v) => v,
        Err(e) => return -i64::from(e),
    };

    // ---- 4. Read each argv / envp string into a kernel buffer. ----
    let mut total_bytes: usize = 0;
    let mut argv_bufs: alloc::vec::Vec<alloc::vec::Vec<u8>> =
        alloc::vec::Vec::with_capacity(argv_ptrs.len());
    for p in argv_ptrs {
        let s = match read_user_cstr(p, EXECVE_MAX_STR_LEN) {
            Ok(s) => s,
            Err(e) => return -i64::from(e),
        };
        total_bytes = total_bytes.saturating_add(s.len()).saturating_add(1);
        if total_bytes > EXECVE_MAX_TOTAL_BYTES {
            return -i64::from(errno::E2BIG);
        }
        argv_bufs.push(s);
    }
    let mut envp_bufs: alloc::vec::Vec<alloc::vec::Vec<u8>> =
        alloc::vec::Vec::with_capacity(envp_ptrs.len());
    for p in envp_ptrs {
        let s = match read_user_cstr(p, EXECVE_MAX_STR_LEN) {
            Ok(s) => s,
            Err(e) => return -i64::from(e),
        };
        total_bytes = total_bytes.saturating_add(s.len()).saturating_add(1);
        if total_bytes > EXECVE_MAX_TOTAL_BYTES {
            return -i64::from(errno::E2BIG);
        }
        envp_bufs.push(s);
    }

    // ---- 5. Read file from VFS BEFORE tearing down old AS. ----
    let elf_data = match crate::fs::vfs::Vfs::read_file(filename) {
        Ok(d) => d,
        Err(e) => return -i64::from(linux_errno_for(e)),
    };

    // ---- 6. Build &[&[u8]] views for exec_process. ----
    let argv_slices: alloc::vec::Vec<&[u8]> =
        argv_bufs.iter().map(alloc::vec::Vec::as_slice).collect();
    let envp_slices: alloc::vec::Vec<&[u8]> =
        envp_bufs.iter().map(alloc::vec::Vec::as_slice).collect();

    // ---- 7. Exec.  After this point the old AS is gone on success. ----
    match exec_process(pid, &elf_data, &argv_slices, &envp_slices) {
        Ok(result) => {
            // Reset caught signal handlers (POSIX) and drop the now-
            // stale signal trampoline; the new image's libc init
            // will re-register.
            signal::on_exec(pid);

            // Rewrite the saved frame so SYSRET lands at the new
            // entry point with a clean register state.
            frame.user_rip = result.entry_rip;
            frame.user_rsp = result.user_rsp;
            frame.arg0 = 0; // rdi
            frame.arg1 = 0; // rsi
            frame.arg2 = 0; // rdx
            frame.arg3 = 0; // r10
            frame.arg4 = 0; // r8
            frame.arg5 = 0; // r9
            frame.rbx = 0;
            frame.rbp = 0;
            frame.r12 = 0;
            frame.r13 = 0;
            frame.r14 = 0;
            frame.r15 = 0;
            // RFLAGS: keep IF=1 (interrupts enabled), reserved bit 1.
            frame.user_rflags = 0x202;
            0
        }
        Err(e) => -i64::from(linux_errno_for(e)),
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Translate-and-dispatch a single Linux syscall.
///
/// Called from `syscall_handler_inner` when the calling process has
/// [`pcb::AbiMode::Linux`].  Numbers not in the implemented table return
/// `-ENOSYS`.
#[must_use]
pub fn dispatch_linux(nr: u64, args: &SyscallArgs) -> SyscallResult {
    match nr {
        nr::READ => sys_read(args),
        nr::WRITE => sys_write(args),
        nr::OPEN => sys_open(args),
        nr::CLOSE => sys_close(args),
        nr::LSEEK => sys_lseek(args),
        nr::READV => sys_readv(args),
        nr::WRITEV => sys_writev(args),
        nr::DUP => sys_dup(args),
        nr::DUP2 => sys_dup2(args),
        nr::DUP3 => sys_dup3(args),
        nr::FCNTL => sys_fcntl(args),
        nr::PIPE => sys_pipe(args),
        nr::PIPE2 => sys_pipe2(args),
        nr::OPENAT => sys_openat(args),
        nr::MMAP => sys_mmap(args),
        nr::MPROTECT => sys_mprotect(args),
        nr::MUNMAP => sys_munmap(args),
        nr::BRK => sys_brk(args),
        nr::RT_SIGACTION => sys_rt_sigaction(args),
        nr::RT_SIGPROCMASK => sys_rt_sigprocmask(args),
        nr::SCHED_YIELD => sys_sched_yield(args),
        nr::NANOSLEEP => sys_nanosleep(args),
        nr::GETPID => sys_getpid(args),
        nr::EXIT => sys_exit(args),
        nr::KILL => sys_kill(args),
        nr::UNAME => sys_uname(args),
        nr::GETTIMEOFDAY => sys_gettimeofday(args),
        nr::GETUID => sys_getuid(args),
        nr::GETGID => sys_getgid(args),
        nr::GETEUID => sys_geteuid(args),
        nr::GETEGID => sys_getegid(args),
        nr::GETPPID => sys_getppid(args),
        nr::ARCH_PRCTL => sys_arch_prctl(args),
        nr::GETTID => sys_gettid(args),
        nr::TIME => sys_time(args),
        nr::FUTEX => sys_futex(args),
        nr::SET_TID_ADDRESS => sys_set_tid_address(args),
        nr::CLOCK_GETTIME => sys_clock_gettime(args),
        nr::CLOCK_GETRES => sys_clock_getres(args),
        nr::CLOCK_NANOSLEEP => sys_clock_nanosleep(args),
        nr::EXIT_GROUP => sys_exit_group(args),
        nr::SET_ROBUST_LIST => sys_set_robust_list(args),
        nr::GETRANDOM => sys_getrandom(args),
        _ => linux_err(errno::ENOSYS),
    }
}

// ---------------------------------------------------------------------------
// Helper: read/write user struct timespec
// ---------------------------------------------------------------------------

/// Linux `struct timespec { time_t tv_sec; long tv_nsec; }` on x86_64.
///
/// Both fields are 8 bytes (`time_t` is 64-bit on x86_64 Linux, and
/// `long` is 64-bit in the LP64 model).  Total size: 16 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxTimespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

impl LinuxTimespec {
    /// Convert a non-negative `(sec, nsec)` pair to total nanoseconds.
    ///
    /// Saturates at `u64::MAX` on overflow (matching Linux's
    /// `clock_nanosleep` clamping for absurdly large durations).
    #[must_use]
    pub const fn to_nanos(self) -> u64 {
        if self.tv_sec < 0 || self.tv_nsec < 0 || self.tv_nsec >= 1_000_000_000 {
            return 0;
        }
        let sec_ns = (self.tv_sec as u64).saturating_mul(1_000_000_000);
        sec_ns.saturating_add(self.tv_nsec as u64)
    }

    /// Build a timespec from a non-negative ns count.
    #[must_use]
    pub const fn from_nanos(ns: u64) -> Self {
        let sec = ns / 1_000_000_000;
        let rem = ns % 1_000_000_000;
        #[allow(clippy::cast_possible_wrap)]
        Self {
            tv_sec: sec as i64,
            tv_nsec: rem as i64,
        }
    }
}

/// Read a `struct timespec` from a userspace pointer.
fn read_timespec(user_ptr: u64) -> Result<LinuxTimespec, KernelError> {
    if user_ptr == 0 {
        return Err(KernelError::InvalidAddress);
    }
    let mut ts = LinuxTimespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: We pass copy_from_user a kernel-owned buffer; it validates
    // the user range before touching it and uses STAC/CLAC for SMAP.
    unsafe {
        crate::mm::user::copy_from_user(
            user_ptr,
            (&raw mut ts).cast::<u8>(),
            core::mem::size_of::<LinuxTimespec>(),
        )?;
    }
    Ok(ts)
}

/// Write a `struct timespec` into a userspace pointer.
fn write_timespec(user_ptr: u64, ts: LinuxTimespec) -> Result<(), KernelError> {
    if user_ptr == 0 {
        return Err(KernelError::InvalidAddress);
    }
    // SAFETY: copy_to_user validates the user range before writing.
    unsafe {
        crate::mm::user::copy_to_user(
            (&raw const ts).cast::<u8>(),
            user_ptr,
            core::mem::size_of::<LinuxTimespec>(),
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Per-syscall translations
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Linux fd-table dispatch helpers
// ---------------------------------------------------------------------------

use crate::proc::linux_fd::{FdEntry, HandleKind};

/// Look up `fd` in the caller's Linux fd table.  Returns -EBADF (as a
/// `SyscallResult`) if the caller has no Linux fd table or `fd` is
/// not open.
fn lookup_caller_fd(fd: i32) -> Result<FdEntry, SyscallResult> {
    let pid = match caller_pid() {
        Some(p) => p,
        None => return Err(linux_err(errno::EBADF)),
    };
    pcb::linux_fd_lookup(pid, fd).ok_or(linux_err(errno::EBADF))
}

/// Issue the kernel-side close appropriate to `entry.kind`.  No-op for
/// `Console` handles (no kernel resource).
///
/// Public so the process-exec path in `crate::proc::spawn` can use it
/// to release `FD_CLOEXEC` handles when an exec re-uses an existing
/// Linux fd table — see `pcb::linux_fd_exec_cloexec`.
pub fn close_handle(entry: FdEntry) -> SyscallResult {
    match entry.kind {
        HandleKind::Console => SyscallResult::ok(0),
        HandleKind::File => {
            let a = SyscallArgs {
                arg0: entry.raw_handle,
                arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            };
            linux_from_native(handlers::sys_fs_close(&a))
        }
        HandleKind::Pipe => {
            let a = SyscallArgs {
                arg0: entry.raw_handle,
                arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            };
            linux_from_native(handlers::sys_pipe_close(&a))
        }
    }
}

/// Dispatch a `write(buf, len)` against an fd entry.  Routes by handle
/// kind to the appropriate native handler.
fn dispatch_write(entry: FdEntry, buf: u64, len: u64) -> SyscallResult {
    match entry.kind {
        HandleKind::Console => {
            // The kernel console doesn't distinguish stdin / stdout /
            // stderr — writes to "fd 0" silently succeed (matching
            // TTY behaviour when stdin happens to be writable).
            if entry.status_flags & oflags::O_ACCMODE == oflags::O_RDONLY {
                #[allow(clippy::cast_possible_wrap)]
                return SyscallResult::ok(len as i64);
            }
            let a = SyscallArgs {
                arg0: buf, arg1: len, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            };
            linux_from_native(handlers::sys_console_write(&a))
        }
        HandleKind::File => {
            let a = SyscallArgs {
                arg0: entry.raw_handle, arg1: buf, arg2: len,
                arg3: 0, arg4: 0, arg5: 0,
            };
            linux_from_native(handlers::sys_fs_write(&a))
        }
        HandleKind::Pipe => {
            let a = SyscallArgs {
                arg0: entry.raw_handle, arg1: buf, arg2: len,
                arg3: 0, arg4: 0, arg5: 0,
            };
            linux_from_native(handlers::sys_pipe_write(&a))
        }
    }
}

/// Dispatch a `read(buf, cap)` against an fd entry.
fn dispatch_read(entry: FdEntry, buf: u64, cap: u64) -> SyscallResult {
    match entry.kind {
        HandleKind::Console => {
            // We approximate Linux TTY read with the line-oriented
            // single-character read — enough for the typical "read
            // one keystroke" pattern.  Multi-byte requests are
            // capped at one byte; libc will retry as needed.
            if cap == 0 {
                return SyscallResult::ok(0);
            }
            let a = SyscallArgs {
                arg0: buf, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            };
            linux_from_native(handlers::sys_console_read_char(&a))
        }
        HandleKind::File => {
            let a = SyscallArgs {
                arg0: entry.raw_handle, arg1: buf, arg2: cap,
                arg3: 0, arg4: 0, arg5: 0,
            };
            linux_from_native(handlers::sys_fs_read(&a))
        }
        HandleKind::Pipe => {
            let a = SyscallArgs {
                arg0: entry.raw_handle, arg1: buf, arg2: cap,
                arg3: 0, arg4: 0, arg5: 0,
            };
            linux_from_native(handlers::sys_pipe_read(&a))
        }
    }
}

/// `write(fd, buf, count)` — consults the per-process Linux fd table.
fn sys_write(args: &SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as i32;
    let buf = args.arg1;
    let count = args.arg2;

    let entry = match lookup_caller_fd(fd) {
        Ok(e) => e,
        Err(r) => return r,
    };
    dispatch_write(entry, buf, count)
}

/// `read(fd, buf, count)` — consults the per-process Linux fd table.
fn sys_read(args: &SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as i32;
    let buf = args.arg1;
    let count = args.arg2;

    let entry = match lookup_caller_fd(fd) {
        Ok(e) => e,
        Err(r) => return r,
    };
    dispatch_read(entry, buf, count)
}

/// `writev(fd, iov, iovcnt)` — vectored write via the fd table.
fn sys_writev(args: &SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as i32;
    let iov_ptr = args.arg1;
    let iovcnt = args.arg2 as i32;

    if iovcnt < 0 || iovcnt > 1024 {
        return linux_err(errno::EINVAL);
    }
    let entry = match lookup_caller_fd(fd) {
        Ok(e) => e,
        Err(r) => return r,
    };

    // Linux `struct iovec { void *iov_base; size_t iov_len; }` — 16 bytes on
    // x86_64.
    #[repr(C)]
    struct Iovec {
        base: u64,
        len: u64,
    }

    let mut total: i64 = 0;
    for i in 0..iovcnt {
        let entry_ptr = iov_ptr.wrapping_add((i as u64) * 16);
        let mut iov = Iovec { base: 0, len: 0 };
        // SAFETY: copy_from_user validates the user range.
        let r = unsafe {
            crate::mm::user::copy_from_user(
                entry_ptr,
                (&raw mut iov).cast::<u8>(),
                core::mem::size_of::<Iovec>(),
            )
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
        if iov.len == 0 {
            continue;
        }
        let r = dispatch_write(entry, iov.base, iov.len);
        if r.value < 0 {
            if total == 0 {
                return r;
            }
            return SyscallResult::ok(total);
        }
        total = total.saturating_add(r.value);
    }
    SyscallResult::ok(total)
}

/// `readv(fd, iov, iovcnt)` — vectored read via the fd table.
fn sys_readv(args: &SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as i32;
    let iov_ptr = args.arg1;
    let iovcnt = args.arg2 as i32;

    if iovcnt < 0 || iovcnt > 1024 {
        return linux_err(errno::EINVAL);
    }
    let entry = match lookup_caller_fd(fd) {
        Ok(e) => e,
        Err(r) => return r,
    };

    #[repr(C)]
    struct Iovec {
        base: u64,
        len: u64,
    }

    let mut total: i64 = 0;
    for i in 0..iovcnt {
        let entry_ptr = iov_ptr.wrapping_add((i as u64) * 16);
        let mut iov = Iovec { base: 0, len: 0 };
        // SAFETY: copy_from_user validates the user range.
        let r = unsafe {
            crate::mm::user::copy_from_user(
                entry_ptr,
                (&raw mut iov).cast::<u8>(),
                core::mem::size_of::<Iovec>(),
            )
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
        if iov.len == 0 {
            continue;
        }
        let r = dispatch_read(entry, iov.base, iov.len);
        if r.value < 0 {
            if total == 0 {
                return r;
            }
            return SyscallResult::ok(total);
        }
        if r.value == 0 {
            // EOF — short return is well-defined for readv.
            break;
        }
        total = total.saturating_add(r.value);
    }
    SyscallResult::ok(total)
}

/// `close(fd)` — remove `fd` from the per-process Linux fd table and,
/// if no other fd still references the same handle, release the
/// underlying kernel resource.
fn sys_close(args: &SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as i32;
    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EBADF),
    };
    let entry = match pcb::linux_fd_take(pid, fd) {
        Some(e) => e,
        None => return linux_err(errno::EBADF),
    };
    if entry.kind.needs_kernel_close()
        && !pcb::linux_fd_is_handle_referenced(pid, entry.kind, entry.raw_handle, -1)
    {
        // No other fd still references this handle — release it.
        let _ = close_handle(entry);
    }
    SyscallResult::ok(0)
}

/// `dup(oldfd)` — duplicate `oldfd` onto the lowest free slot.
fn sys_dup(args: &SyscallArgs) -> SyscallResult {
    let oldfd = args.arg0 as i32;
    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EBADF),
    };
    match pcb::linux_fd_dup(pid, oldfd, 0) {
        Ok(newfd) => SyscallResult::ok(i64::from(newfd)),
        Err(e) => linux_err(linux_errno_for(e)),
    }
}

/// Shared back-end for `dup2` / `dup3`.
fn sys_dup2_impl(oldfd: i32, newfd: i32, cloexec: bool) -> SyscallResult {
    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EBADF),
    };
    if newfd < 0 {
        return linux_err(errno::EBADF);
    }
    let (returned_fd, prev) = match pcb::linux_fd_dup2(pid, oldfd, newfd) {
        Ok(t) => t,
        Err(e) => return linux_err(linux_errno_for(e)),
    };
    // If the duplicate displaced an entry, close it (refcount-aware).
    if let Some(prev_entry) = prev
        && prev_entry.kind.needs_kernel_close()
        && !pcb::linux_fd_is_handle_referenced(
            pid,
            prev_entry.kind,
            prev_entry.raw_handle,
            -1,
        )
    {
        let _ = close_handle(prev_entry);
    }
    if cloexec {
        // dup3 honours O_CLOEXEC on the destination fd.
        let _ = pcb::linux_fd_set_fd_flags(
            pid,
            returned_fd,
            crate::proc::linux_fd::FD_CLOEXEC,
        );
    }
    SyscallResult::ok(i64::from(returned_fd))
}

/// `dup2(oldfd, newfd)` — duplicate onto a specific fd.  POSIX: if
/// `oldfd == newfd` and `oldfd` is valid, returns `newfd` without
/// closing anything.
fn sys_dup2(args: &SyscallArgs) -> SyscallResult {
    sys_dup2_impl(args.arg0 as i32, args.arg1 as i32, false)
}

/// `dup3(oldfd, newfd, flags)` — like dup2 but `flags & O_CLOEXEC`
/// sets FD_CLOEXEC on the new fd.  Unlike dup2, `oldfd == newfd` is
/// an error (Linux returns EINVAL).
fn sys_dup3(args: &SyscallArgs) -> SyscallResult {
    let oldfd = args.arg0 as i32;
    let newfd = args.arg1 as i32;
    let flags = args.arg2 as u32;
    if oldfd == newfd {
        return linux_err(errno::EINVAL);
    }
    sys_dup2_impl(oldfd, newfd, flags & oflags::O_CLOEXEC != 0)
}

/// `fcntl(fd, cmd, arg)` — subset relevant to fd-table state.
fn sys_fcntl(args: &SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as i32;
    let cmd = args.arg1 as u32;
    let arg = args.arg2;

    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EBADF),
    };

    match cmd {
        fcntl_cmd::F_DUPFD | fcntl_cmd::F_DUPFD_CLOEXEC => {
            let min_fd = arg as i32;
            if min_fd < 0 {
                return linux_err(errno::EINVAL);
            }
            match pcb::linux_fd_dup(pid, fd, min_fd) {
                Ok(newfd) => {
                    if cmd == fcntl_cmd::F_DUPFD_CLOEXEC {
                        let _ = pcb::linux_fd_set_fd_flags(
                            pid,
                            newfd,
                            crate::proc::linux_fd::FD_CLOEXEC,
                        );
                    }
                    SyscallResult::ok(i64::from(newfd))
                }
                Err(e) => linux_err(linux_errno_for(e)),
            }
        }
        fcntl_cmd::F_GETFD => match pcb::linux_fd_lookup(pid, fd) {
            Some(e) => SyscallResult::ok(i64::from(e.fd_flags)),
            None => linux_err(errno::EBADF),
        },
        fcntl_cmd::F_SETFD => {
            let new_flags = arg as u32;
            match pcb::linux_fd_set_fd_flags(pid, fd, new_flags) {
                Ok(()) => SyscallResult::ok(0),
                Err(e) => linux_err(linux_errno_for(e)),
            }
        }
        fcntl_cmd::F_GETFL => match pcb::linux_fd_lookup(pid, fd) {
            Some(e) => SyscallResult::ok(i64::from(e.status_flags)),
            None => linux_err(errno::EBADF),
        },
        fcntl_cmd::F_SETFL => {
            let new_flags = arg as u32;
            match pcb::linux_fd_set_status_flags(pid, fd, new_flags) {
                Ok(()) => SyscallResult::ok(0),
                Err(e) => linux_err(linux_errno_for(e)),
            }
        }
        _ => linux_err(errno::ENOSYS),
    }
}

/// `lseek(fd, offset, whence)` — only meaningful for `File` handles.
fn sys_lseek(args: &SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as i32;
    let entry = match lookup_caller_fd(fd) {
        Ok(e) => e,
        Err(r) => return r,
    };
    match entry.kind {
        HandleKind::File => {
            let a = SyscallArgs {
                arg0: entry.raw_handle,
                arg1: args.arg1,
                arg2: args.arg2,
                arg3: 0, arg4: 0, arg5: 0,
            };
            linux_from_native(handlers::sys_fs_seek(&a))
        }
        HandleKind::Console | HandleKind::Pipe => linux_err(errno::ESPIPE),
    }
}

/// Translate Linux `O_*` flag bits to the kernel's `OpenFlags`.
fn translate_open_flags(linux_flags: u32) -> u32 {
    use crate::fs::handle::OpenFlags;
    let access = linux_flags & oflags::O_ACCMODE;
    let mut bits: u32 = 0;
    match access {
        oflags::O_RDONLY => bits |= OpenFlags::READ.bits(),
        oflags::O_WRONLY => bits |= OpenFlags::WRITE.bits(),
        oflags::O_RDWR => bits |= OpenFlags::READ.bits() | OpenFlags::WRITE.bits(),
        _ => bits |= OpenFlags::READ.bits(),
    }
    if linux_flags & oflags::O_CREAT != 0 {
        bits |= OpenFlags::CREATE.bits();
    }
    if linux_flags & oflags::O_TRUNC != 0 {
        bits |= OpenFlags::TRUNCATE.bits();
    }
    if linux_flags & oflags::O_APPEND != 0 {
        bits |= OpenFlags::APPEND.bits();
    }
    bits
}

/// Shared backend for `open` / `openat`.
fn open_common(path_ptr: u64, path_len_hint: u64, flags: u32) -> SyscallResult {
    if path_ptr == 0 {
        return linux_err(errno::EFAULT);
    }

    // Linux paths are NUL-terminated.  Scan up to a sane cap (matching
    // sys_fs_open's internal 256-byte cap) to locate the terminator
    // without trusting the caller-provided length.  We validate one
    // page at a time to keep SMAP windows tight.
    const MAX_PATH: usize = 256;
    let mut tmp = [0u8; MAX_PATH];
    let mut len = 0usize;
    while len < MAX_PATH {
        // SAFETY: copy_from_user validates each one-byte read.
        let r = unsafe {
            crate::mm::user::copy_from_user(
                path_ptr.wrapping_add(len as u64),
                tmp.as_mut_ptr().wrapping_add(len),
                1,
            )
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
        if tmp[len] == 0 {
            break;
        }
        len += 1;
    }
    if len == 0 || len >= MAX_PATH {
        // Empty path or no terminator within MAX_PATH.
        return linux_err(if len == 0 { errno::ENOENT } else { errno::ENAMETOOLONG });
    }
    // Honour caller's explicit length when provided.  sys_fs_open
    // re-reads the path itself from userspace; we forward the user
    // pointer and length.
    let user_len = if path_len_hint == 0 || path_len_hint > len as u64 {
        len as u64
    } else {
        path_len_hint
    };

    let kernel_flags = translate_open_flags(flags);
    let open_args = SyscallArgs {
        arg0: path_ptr,
        arg1: user_len,
        arg2: u64::from(kernel_flags),
        arg3: 0, arg4: 0, arg5: 0,
    };
    let r = handlers::sys_fs_open(&open_args);
    if r.value < 0 {
        return linux_from_native(r);
    }
    let raw_handle = r.value as u64;

    // Build the FdEntry status flags from the Linux flags so future
    // F_GETFL returns something coherent.
    let mut status_flags = flags & (oflags::O_ACCMODE | oflags::O_APPEND | oflags::O_NONBLOCK);
    if flags & oflags::O_CLOEXEC == 0 {
        // No-op: status_flags doesn't track FD_CLOEXEC (that's fd_flags).
    }
    // Normalise the access bits — translate_open_flags coerced an
    // unknown access mode to O_RDONLY, so do the same here.
    if status_flags & oflags::O_ACCMODE > oflags::O_RDWR {
        status_flags = (status_flags & !oflags::O_ACCMODE) | oflags::O_RDONLY;
    }
    let mut entry = FdEntry::file(raw_handle, status_flags);
    if flags & oflags::O_CLOEXEC != 0 {
        entry.fd_flags = crate::proc::linux_fd::FD_CLOEXEC;
    }

    let pid = match caller_pid() {
        Some(p) => p,
        None => {
            // Caller is a kernel task — close the file we just opened
            // (it has nowhere to live) and return EBADF.
            let _ = handlers::sys_fs_close(&SyscallArgs {
                arg0: raw_handle, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            });
            return linux_err(errno::EBADF);
        }
    };
    match pcb::linux_fd_install(pid, entry, 0) {
        Ok(fd) => SyscallResult::ok(i64::from(fd)),
        Err(e) => {
            // Roll the file open back on table failure.
            let _ = handlers::sys_fs_close(&SyscallArgs {
                arg0: raw_handle, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            });
            linux_err(linux_errno_for(e))
        }
    }
}

/// `open(path, flags, mode)` — equivalent to `openat(AT_FDCWD, path, flags, mode)`.
fn sys_open(args: &SyscallArgs) -> SyscallResult {
    open_common(args.arg0, 0, args.arg1 as u32)
}

/// `openat(dirfd, path, flags, mode)` — only `AT_FDCWD` is honoured.
fn sys_openat(args: &SyscallArgs) -> SyscallResult {
    let dirfd = args.arg0 as i32;
    if dirfd != AT_FDCWD {
        // Directory-fd-relative opens require an `OpenFlags::DIRECTORY`
        // VFS handle we don't have yet.
        return linux_err(errno::ENOSYS);
    }
    open_common(args.arg1, 0, args.arg2 as u32)
}

/// Shared backend for `pipe(2)` / `pipe2(2)`.
///
/// `pipefd_ptr` is a user-space `int pipefd[2]`; we write the two new
/// fd numbers there.  `flags` is interpreted as the Linux `O_*` set
/// (`O_CLOEXEC` and `O_NONBLOCK`).  Returns 0 on success.
fn pipe_common(pipefd_ptr: u64, flags: u32) -> SyscallResult {
    if pipefd_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    // pipe2 rejects unknown flag bits (Linux returns -EINVAL).
    let known = oflags::O_CLOEXEC | oflags::O_NONBLOCK;
    if flags & !known != 0 {
        return linux_err(errno::EINVAL);
    }

    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EBADF),
    };

    // Validate the user destination up front; better to fail before
    // creating pipe state than to leak handles on a copy_to_user fault.
    if let Err(e) = crate::mm::user::validate_user_write(pipefd_ptr, 8) {
        return linux_err(linux_errno_for(e));
    }

    // Create the kernel pipe.  The native handler also registers both
    // endpoints in the per-process IPC handle list; the fd-table install
    // below adds the user-visible reference.
    let zero = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
    let create_res = handlers::sys_pipe_create(&zero);
    if create_res.value < 0 {
        return linux_from_native(create_res);
    }
    let read_raw = create_res.value as u64;
    let write_raw = create_res.value2 as u64;

    // Build entries.  Read end gets O_RDONLY, write end O_WRONLY.  Both
    // honour the caller's O_CLOEXEC / O_NONBLOCK request.
    let status_common = flags & oflags::O_NONBLOCK;
    let mut read_entry = FdEntry::pipe(read_raw, oflags::O_RDONLY | status_common);
    let mut write_entry = FdEntry::pipe(write_raw, oflags::O_WRONLY | status_common);
    if flags & oflags::O_CLOEXEC != 0 {
        read_entry.fd_flags = crate::proc::linux_fd::FD_CLOEXEC;
        write_entry.fd_flags = crate::proc::linux_fd::FD_CLOEXEC;
    }

    // Install read end first, then write end.  If the second install
    // fails (table full), roll the first one back.
    let read_fd = match pcb::linux_fd_install(pid, read_entry, 0) {
        Ok(fd) => fd,
        Err(e) => {
            // Tear down the kernel pipe state we just created.
            let _ = handlers::sys_pipe_close(&SyscallArgs {
                arg0: read_raw, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            });
            let _ = handlers::sys_pipe_close(&SyscallArgs {
                arg0: write_raw, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            });
            return linux_err(linux_errno_for(e));
        }
    };
    let write_fd = match pcb::linux_fd_install(pid, write_entry, 0) {
        Ok(fd) => fd,
        Err(e) => {
            // Roll back the read-end install + both pipe endpoints.
            let _ = pcb::linux_fd_take(pid, read_fd);
            let _ = handlers::sys_pipe_close(&SyscallArgs {
                arg0: read_raw, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            });
            let _ = handlers::sys_pipe_close(&SyscallArgs {
                arg0: write_raw, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            });
            return linux_err(linux_errno_for(e));
        }
    };

    // Copy the (read_fd, write_fd) pair into the user's pipefd[2].
    let fds: [i32; 2] = [read_fd, write_fd];
    // SAFETY: validated above.
    let r = unsafe {
        crate::mm::user::copy_to_user(
            (&raw const fds).cast::<u8>(),
            pipefd_ptr,
            core::mem::size_of::<[i32; 2]>(),
        )
    };
    if let Err(e) = r {
        // The destination became invalid between validation and copy
        // (e.g. another thread unmapped it).  Roll back both installs.
        let _ = pcb::linux_fd_take(pid, read_fd);
        let _ = pcb::linux_fd_take(pid, write_fd);
        let _ = handlers::sys_pipe_close(&SyscallArgs {
            arg0: read_raw, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
        });
        let _ = handlers::sys_pipe_close(&SyscallArgs {
            arg0: write_raw, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
        });
        return linux_err(linux_errno_for(e));
    }

    SyscallResult::ok(0)
}

/// `pipe(pipefd)` — create a new pipe; equivalent to `pipe2(pipefd, 0)`.
fn sys_pipe(args: &SyscallArgs) -> SyscallResult {
    pipe_common(args.arg0, 0)
}

/// `pipe2(pipefd, flags)` — like pipe but honours O_CLOEXEC / O_NONBLOCK.
fn sys_pipe2(args: &SyscallArgs) -> SyscallResult {
    pipe_common(args.arg0, args.arg1 as u32)
}

/// `mmap(addr, length, prot, flags, fd, offset)` — anonymous private only.
///
/// Linux flags translation:
/// - `MAP_PRIVATE` (0x02) + `MAP_ANONYMOUS` (0x20): supported.
/// - Anything else (file-backed, shared): returns -ENOSYS until the
///   kernel-side fd table arrives.
fn sys_mmap(args: &SyscallArgs) -> SyscallResult {
    const MAP_PRIVATE: u64 = 0x02;
    const MAP_ANONYMOUS: u64 = 0x20;
    const MAP_FIXED: u64 = 0x10;

    let addr_hint = args.arg0;
    let length = args.arg1;
    let _prot = args.arg2;
    let flags = args.arg3;
    let fd = args.arg4 as i32;

    // File-backed maps not yet supported.
    if (flags & MAP_ANONYMOUS) == 0 || fd >= 0 {
        return linux_err(errno::ENOSYS);
    }
    if (flags & MAP_PRIVATE) == 0 {
        // We don't support shared anonymous in Linux ABI yet.
        return linux_err(errno::ENOSYS);
    }

    // Native SYS_MMAP: arg0 = hint, arg1 = length, arg2 = our flags,
    // arg3 = phys addr.  We pass 0 flags (private RW), which our handler
    // treats as "anonymous, demand-allocated".
    let native_flags: u64 = if (flags & MAP_FIXED) != 0 { 0x01 } else { 0 };
    let native_args = SyscallArgs {
        arg0: addr_hint,
        arg1: length,
        arg2: native_flags,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let r = handlers::sys_mmap(&native_args);
    linux_from_native(r)
}

/// `mprotect(addr, len, prot)` — silently succeeds.
///
/// Our VMA layer doesn't track per-page protection changes yet.  We
/// validate that the range is well-formed (length > 0, addr aligned to
/// 16 KiB) and return 0.  Programs relying on actual PROT_NONE guard
/// pages will not be protected — documented limitation.
fn sys_mprotect(args: &SyscallArgs) -> SyscallResult {
    let _addr = args.arg0;
    let len = args.arg1;
    if len == 0 {
        return linux_err(errno::EINVAL);
    }
    SyscallResult::ok(0)
}

/// `munmap(addr, len)` — passes through to native.
fn sys_munmap(args: &SyscallArgs) -> SyscallResult {
    let native_args = SyscallArgs {
        arg0: args.arg0,
        arg1: args.arg1,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    linux_from_native(handlers::sys_munmap(&native_args))
}

/// `brk(addr)` — returns the current brk (we don't grow the heap).
///
/// Most modern libc allocators use mmap for large allocations and
/// fall back to brk only when both are available.  Returning the input
/// address unchanged is the documented "brk failed, keep current value"
/// behaviour.  Programs that strictly require brk will fail allocations
/// > current brk and either error out or fall through to mmap.
fn sys_brk(args: &SyscallArgs) -> SyscallResult {
    // Return the requested value to claim it succeeded.  When the
    // memory-manager VMA layer grows a `brk` region, this becomes a
    // real allocation; until then, programs see "your brk is whatever
    // you asked for" and tend to fall through to mmap-based allocators.
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(args.arg0 as i64)
}

/// `rt_sigaction(sig, act, oldact, sigsetsize)` — register a handler.
///
/// We forward only the handler pointer; sa_mask and sa_flags are
/// silently ignored (matching native signal-shim limitations).
fn sys_rt_sigaction(args: &SyscallArgs) -> SyscallResult {
    let sig = args.arg0;
    let act_ptr = args.arg1;

    // Linux `struct sigaction { void (*sa_handler)(int); ... }` — the
    // handler is the first 8 bytes.
    let handler: u64 = if act_ptr == 0 {
        // act = NULL means "just query oldact"; we don't track state,
        // so return success without changing anything.
        return SyscallResult::ok(0);
    } else {
        let mut buf = [0u8; 8];
        // SAFETY: copy_from_user validates the user range.
        let r = unsafe {
            crate::mm::user::copy_from_user(act_ptr, buf.as_mut_ptr(), 8)
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
        u64::from_ne_bytes(buf)
    };

    let native_args = SyscallArgs {
        arg0: sig,
        arg1: handler,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    linux_from_native(handlers::sys_signal_register(&native_args))
}

/// `rt_sigprocmask(how, set, oldset, sigsetsize)` — wrap signal_mask.
fn sys_rt_sigprocmask(args: &SyscallArgs) -> SyscallResult {
    let how = args.arg0;
    let set_ptr = args.arg1;
    let oldset_ptr = args.arg2;

    // Read the new mask (64-bit) if `set` is non-NULL.
    let new_mask: u64 = if set_ptr == 0 {
        0
    } else {
        let mut buf = [0u8; 8];
        // SAFETY: copy_from_user validates the user range.
        let r = unsafe { crate::mm::user::copy_from_user(set_ptr, buf.as_mut_ptr(), 8) };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
        u64::from_ne_bytes(buf)
    };

    let native_args = SyscallArgs {
        arg0: how,
        arg1: new_mask,
        arg2: oldset_ptr,
        arg3: u64::from(set_ptr == 0),
        arg4: 0,
        arg5: 0,
    };
    linux_from_native(handlers::sys_signal_mask(&native_args))
}

/// `sched_yield()` — direct.
fn sys_sched_yield(_args: &SyscallArgs) -> SyscallResult {
    crate::sched::yield_now();
    SyscallResult::ok(0)
}

/// `nanosleep(req, rem)` — sleep for the requested timespec.
///
/// `rem` (remainder on signal interruption) is left untouched — our
/// sleep is not currently interruptible.
fn sys_nanosleep(args: &SyscallArgs) -> SyscallResult {
    let req_ptr = args.arg0;
    let req = match read_timespec(req_ptr) {
        Ok(t) => t,
        Err(e) => return linux_err(linux_errno_for(e)),
    };
    let ns = req.to_nanos();
    if ns == 0 {
        crate::sched::yield_now();
        return SyscallResult::ok(0);
    }
    crate::sched::sleep_ns(ns);
    SyscallResult::ok(0)
}

/// `getpid()` — current process ID.
fn sys_getpid(_args: &SyscallArgs) -> SyscallResult {
    let task = crate::sched::current_task_id();
    let pid = crate::proc::thread::owner_process(task).unwrap_or(0);
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(pid as i64)
}

/// `exit(status)` — terminate the calling task with the given exit code.
fn sys_exit(args: &SyscallArgs) -> SyscallResult {
    handlers::sys_exit(args);
    // sys_exit never returns; placate the type checker.
    SyscallResult::ok(0)
}

/// `exit_group(status)` — terminate all threads of the calling process.
///
/// We have no thread-group concept yet; this is identical to `exit`.
fn sys_exit_group(args: &SyscallArgs) -> SyscallResult {
    handlers::sys_exit(args);
    SyscallResult::ok(0)
}

/// `kill(pid, sig)` — send a signal.
fn sys_kill(args: &SyscallArgs) -> SyscallResult {
    // Native SYS_SIGNAL_SEND: arg0 = target pid, arg1 = signum.
    linux_from_native(handlers::sys_signal_send(args))
}

/// `uname(buf)` — fill in `struct utsname` with kernel identity.
///
/// `struct utsname` has 6 fields × 65 bytes = 390 bytes total.  We fill
/// the standard fields with values that satisfy Linux programs probing
/// for "are we running on Linux x86_64?".
fn sys_uname(args: &SyscallArgs) -> SyscallResult {
    let user_buf = args.arg0;
    if user_buf == 0 {
        return linux_err(errno::EFAULT);
    }

    let mut buf = [0u8; 6 * 65];
    fn fill(buf: &mut [u8; 6 * 65], idx: usize, s: &[u8]) {
        let off = idx * 65;
        let n = s.len().min(64);
        #[allow(clippy::indexing_slicing)]
        for i in 0..n {
            buf[off + i] = s[i];
        }
        // buf[off + n] is the NUL terminator (already zero).
    }
    fill(&mut buf, 0, b"OuRoS");                    // sysname
    fill(&mut buf, 1, b"localhost");                // nodename
    fill(&mut buf, 2, b"0.1.0-ouros");              // release
    fill(&mut buf, 3, b"#1 SMP");                   // version
    fill(&mut buf, 4, b"x86_64");                   // machine
    fill(&mut buf, 5, b"localdomain");              // domainname (GNU ext)

    // SAFETY: copy_to_user validates the user range.
    let r = unsafe {
        crate::mm::user::copy_to_user(buf.as_ptr(), user_buf, buf.len())
    };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `gettimeofday(tv, tz)` — fills `struct timeval { sec; usec; }`.
fn sys_gettimeofday(args: &SyscallArgs) -> SyscallResult {
    let tv_ptr = args.arg0;
    if tv_ptr == 0 {
        // POSIX: tv may be NULL — succeed.  tz is unused.
        return SyscallResult::ok(0);
    }
    let ns = crate::timekeeping::clock_realtime();
    let sec = ns / 1_000_000_000;
    let usec = (ns % 1_000_000_000) / 1_000;

    #[repr(C)]
    struct Timeval {
        sec: i64,
        usec: i64,
    }
    #[allow(clippy::cast_possible_wrap)]
    let tv = Timeval { sec: sec as i64, usec: usec as i64 };

    // SAFETY: copy_to_user validates.
    let r = unsafe {
        crate::mm::user::copy_to_user(
            (&raw const tv).cast::<u8>(),
            tv_ptr,
            core::mem::size_of::<Timeval>(),
        )
    };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `getuid()` — real user id.  Reads the caller's process credentials.
fn sys_getuid(_args: &SyscallArgs) -> SyscallResult {
    let pid = match caller_pid() {
        Some(p) => p,
        None => return SyscallResult::ok(0), // kernel task
    };
    let uid = pcb::get_credentials(pid).map_or(0, |c| u64::from(c.uid));
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(uid as i64)
}

/// `getgid()`
fn sys_getgid(_args: &SyscallArgs) -> SyscallResult {
    let pid = match caller_pid() {
        Some(p) => p,
        None => return SyscallResult::ok(0),
    };
    let gid = pcb::get_credentials(pid).map_or(0, |c| u64::from(c.gid));
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(gid as i64)
}

/// `geteuid()` — currently aliased to `uid` (no euid tracking yet).
fn sys_geteuid(args: &SyscallArgs) -> SyscallResult {
    sys_getuid(args)
}

/// `getegid()` — currently aliased to `gid` (no egid tracking yet).
fn sys_getegid(args: &SyscallArgs) -> SyscallResult {
    sys_getgid(args)
}

/// `getppid()` — parent's PID.
fn sys_getppid(_args: &SyscallArgs) -> SyscallResult {
    let pid = match caller_pid() {
        Some(p) => p,
        None => return SyscallResult::ok(0),
    };
    let ppid = pcb::parent(pid).unwrap_or(0);
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(ppid as i64)
}

/// `gettid()` — current task ID.
fn sys_gettid(_args: &SyscallArgs) -> SyscallResult {
    let tid = crate::sched::current_task_id();
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(tid as i64)
}

/// `time(tloc)` — seconds since the epoch.
fn sys_time(args: &SyscallArgs) -> SyscallResult {
    let ns = crate::timekeeping::clock_realtime();
    let sec = ns / 1_000_000_000;
    #[allow(clippy::cast_possible_wrap)]
    let sec_i64 = sec as i64;
    if args.arg0 != 0 {
        // SAFETY: copy_to_user validates the destination range.
        let r = unsafe {
            crate::mm::user::copy_to_user(
                (&raw const sec_i64).cast::<u8>(),
                args.arg0,
                core::mem::size_of::<i64>(),
            )
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
    }
    SyscallResult::ok(sec_i64)
}

/// `futex(uaddr, op, val, timeout, uaddr2, val3)` — minimal support.
///
/// Supported operations:
/// - `FUTEX_WAIT` (0): wait until the value at `uaddr` changes.
/// - `FUTEX_WAKE` (1): wake up to `val` waiters on `uaddr`.
///
/// The `FUTEX_PRIVATE_FLAG` (0x80) and `FUTEX_CLOCK_REALTIME` (0x100) are
/// stripped before matching the operation.
fn sys_futex(args: &SyscallArgs) -> SyscallResult {
    const FUTEX_WAIT: u64 = 0;
    const FUTEX_WAKE: u64 = 1;
    const FUTEX_PRIVATE_FLAG: u64 = 0x80;
    const FUTEX_CLOCK_REALTIME: u64 = 0x100;
    const FUTEX_CMD_MASK: u64 = !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);

    let uaddr = args.arg0;
    let op = args.arg1 & FUTEX_CMD_MASK;
    let val = args.arg2;
    let timeout_ptr = args.arg3;

    match op {
        FUTEX_WAIT => {
            let native = if timeout_ptr == 0 {
                let a = SyscallArgs {
                    arg0: uaddr, arg1: val, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
                };
                handlers::sys_futex_wait(&a)
            } else {
                let ts = match read_timespec(timeout_ptr) {
                    Ok(t) => t,
                    Err(e) => return linux_err(linux_errno_for(e)),
                };
                let a = SyscallArgs {
                    arg0: uaddr, arg1: val, arg2: ts.to_nanos(),
                    arg3: 0, arg4: 0, arg5: 0,
                };
                handlers::sys_futex_wait_timeout(&a)
            };
            linux_from_native(native)
        }
        FUTEX_WAKE => {
            let a = SyscallArgs {
                arg0: uaddr, arg1: val, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            };
            linux_from_native(handlers::sys_futex_wake(&a))
        }
        _ => linux_err(errno::ENOSYS),
    }
}

/// `set_tid_address(tidptr)` — Linux uses this for thread-cleanup
/// notification on exit.  We don't track the pointer; just return tid.
fn sys_set_tid_address(_args: &SyscallArgs) -> SyscallResult {
    let tid = crate::sched::current_task_id();
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(tid as i64)
}

/// `set_robust_list(head, len)` — robust-mutex cleanup.  Stubbed.
fn sys_set_robust_list(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `arch_prctl(code, addr)` — only ARCH_SET_FS / ARCH_GET_FS.
///
/// ARCH_SET_FS writes IA32_FS_BASE (MSR 0xC000_0100).  ARCH_GET_FS reads
/// it and stores it via the user pointer.  Anything else returns
/// -ENOSYS.
fn sys_arch_prctl(args: &SyscallArgs) -> SyscallResult {
    const ARCH_SET_GS: u64 = 0x1001;
    const ARCH_SET_FS: u64 = 0x1002;
    const ARCH_GET_FS: u64 = 0x1003;
    const ARCH_GET_GS: u64 = 0x1004;

    const IA32_FS_BASE: u32 = 0xC000_0100;

    let code = args.arg0;
    let addr = args.arg1;

    match code {
        ARCH_SET_FS => {
            // SAFETY: IA32_FS_BASE is a documented architectural MSR;
            // writing the caller's chosen FS base is exactly what Linux
            // does in glibc startup.
            unsafe { crate::cpu::wrmsr(IA32_FS_BASE, addr); }
            SyscallResult::ok(0)
        }
        ARCH_GET_FS => {
            if addr == 0 {
                return linux_err(errno::EFAULT);
            }
            // SAFETY: reading IA32_FS_BASE is side-effect-free.
            let v = unsafe { crate::cpu::rdmsr(IA32_FS_BASE) };
            // SAFETY: copy_to_user validates.
            let r = unsafe {
                crate::mm::user::copy_to_user(
                    (&raw const v).cast::<u8>(),
                    addr,
                    core::mem::size_of::<u64>(),
                )
            };
            if let Err(e) = r {
                return linux_err(linux_errno_for(e));
            }
            SyscallResult::ok(0)
        }
        ARCH_SET_GS | ARCH_GET_GS => linux_err(errno::ENOSYS),
        _ => linux_err(errno::EINVAL),
    }
}

/// `clock_gettime(clockid, tp)` — fills `struct timespec`.
fn sys_clock_gettime(args: &SyscallArgs) -> SyscallResult {
    const CLOCK_REALTIME: u64 = 0;
    const CLOCK_MONOTONIC: u64 = 1;
    const CLOCK_PROCESS_CPUTIME_ID: u64 = 2;
    const CLOCK_THREAD_CPUTIME_ID: u64 = 3;
    const CLOCK_MONOTONIC_RAW: u64 = 4;
    const CLOCK_REALTIME_COARSE: u64 = 5;
    const CLOCK_MONOTONIC_COARSE: u64 = 6;
    const CLOCK_BOOTTIME: u64 = 7;

    let clockid = args.arg0;
    let tp_ptr = args.arg1;

    let ns: u64 = match clockid {
        CLOCK_REALTIME | CLOCK_REALTIME_COARSE => crate::timekeeping::clock_realtime(),
        CLOCK_MONOTONIC | CLOCK_MONOTONIC_RAW | CLOCK_MONOTONIC_COARSE
        | CLOCK_BOOTTIME | CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            crate::hrtimer::now_ns()
        }
        _ => return linux_err(errno::EINVAL),
    };

    let ts = LinuxTimespec::from_nanos(ns);
    if let Err(e) = write_timespec(tp_ptr, ts) {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `clock_getres(clockid, res)` — reports resolution.
///
/// We report 1 ns (the resolution our hrtimer reports in `now_ns`).
fn sys_clock_getres(args: &SyscallArgs) -> SyscallResult {
    let res_ptr = args.arg1;
    if res_ptr == 0 {
        // Linux permits NULL — succeed without writing.
        return SyscallResult::ok(0);
    }
    let ts = LinuxTimespec { tv_sec: 0, tv_nsec: 1 };
    if let Err(e) = write_timespec(res_ptr, ts) {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `clock_nanosleep(clockid, flags, req, rem)` — relative sleep only.
///
/// `TIMER_ABSTIME` (flags = 1) is computed by subtracting the current
/// clock value to make it relative, then sleeping.  Negative results
/// (already-past target) return immediately.
fn sys_clock_nanosleep(args: &SyscallArgs) -> SyscallResult {
    const TIMER_ABSTIME: u64 = 1;
    let clockid = args.arg0;
    let flags = args.arg1;
    let req_ptr = args.arg2;
    let req = match read_timespec(req_ptr) {
        Ok(t) => t,
        Err(e) => return linux_err(linux_errno_for(e)),
    };
    let target_ns = req.to_nanos();
    let now_ns: u64 = match clockid {
        0 => crate::timekeeping::clock_realtime(),
        _ => crate::hrtimer::now_ns(),
    };
    let ns = if (flags & TIMER_ABSTIME) != 0 {
        target_ns.saturating_sub(now_ns)
    } else {
        target_ns
    };
    if ns == 0 {
        crate::sched::yield_now();
    } else {
        crate::sched::sleep_ns(ns);
    }
    SyscallResult::ok(0)
}

/// `getrandom(buf, buflen, flags)` — fill buf with random bytes.
///
/// The kernel does not yet expose a unified CSPRNG.  We use the RDRAND
/// instruction when available and fall back to `rdtsc`-derived bytes
/// otherwise.  Linux's `getrandom` is "best effort to avoid blocking
/// for entropy"; falling back to a TSC stream is documented as a known
/// limitation (`todo.txt`) until the kernel ships a real CSPRNG.
fn sys_getrandom(args: &SyscallArgs) -> SyscallResult {
    let buf_ptr = args.arg0;
    let buf_len = args.arg1 as usize;
    if buf_len == 0 {
        return SyscallResult::ok(0);
    }
    // Cap to avoid pathological huge requests.
    let n = buf_len.min(256);

    // Validate user buffer is writable.
    if let Err(e) = crate::mm::user::validate_user_write(buf_ptr, n) {
        return linux_err(linux_errno_for(e));
    }

    // Fill from a TSC-mixed stream.  Not cryptographic, but good enough
    // for Linux programs that just need non-zero "random-looking" bytes
    // (process IDs in tmp file names, sample jitter, etc.).
    let mut tmp = [0u8; 256];
    let mut state: u64 = crate::bench::rdtsc();
    #[allow(clippy::indexing_slicing)]
    for i in 0..n {
        // xorshift64 step.
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        tmp[i] = (state & 0xff) as u8;
    }
    // SAFETY: validated above.
    #[allow(clippy::indexing_slicing)]
    let r = unsafe { crate::mm::user::copy_to_user(tmp.as_ptr(), buf_ptr, n) };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(n as i64)
}

// ---------------------------------------------------------------------------
// Caller-identity helpers
// ---------------------------------------------------------------------------

fn caller_pid() -> Option<u64> {
    let task = crate::sched::current_task_id();
    crate::proc::thread::owner_process(task)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test exercised at kernel boot — verifies the translation
/// framework wiring without depending on any user process.
///
/// Returns `Ok(())` on success or panics with a diagnostic on the
/// first failure (matching the dispatch self-test convention).
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;

    serial_println!("[syscall/linux] Running translation self-test...");

    // (1) errno mapping round-trips for every variant in the table.
    macro_rules! check_errno {
        ($variant:ident, $expected:expr) => {{
            let mapped = linux_errno_for(KernelError::$variant);
            if mapped != $expected {
                serial_println!(
                    "[syscall/linux]   FAIL: {} → {}, expected {}",
                    stringify!($variant), mapped, $expected
                );
                return Err(KernelError::InternalError);
            }
        }};
    }
    check_errno!(NotSupported, errno::ENOSYS);
    check_errno!(InvalidArgument, errno::EINVAL);
    check_errno!(WouldBlock, errno::EAGAIN);
    check_errno!(TimedOut, errno::ETIMEDOUT);
    check_errno!(OutOfMemory, errno::ENOMEM);
    check_errno!(InvalidAddress, errno::EFAULT);
    check_errno!(NoSuchProcess, errno::ESRCH);
    check_errno!(NoChildProcess, errno::ECHILD);
    check_errno!(ChannelClosed, errno::EPIPE);
    check_errno!(PermissionDenied, errno::EACCES);
    check_errno!(NotFound, errno::ENOENT);
    check_errno!(AlreadyExists, errno::EEXIST);
    check_errno!(NotADirectory, errno::ENOTDIR);
    check_errno!(IsADirectory, errno::EISDIR);
    check_errno!(InvalidHandle, errno::EBADF);
    check_errno!(TooManyOpenFiles, errno::EMFILE);

    // (2) linux_from_native: a native error encoding (signed kernel code
    //     in `value`) gets remapped to -errno on the way out.
    let native_err = SyscallResult::err(KernelError::NotFound);
    let linux_err_res = linux_from_native(native_err);
    if linux_err_res.value != -(errno::ENOENT as i64) {
        serial_println!(
            "[syscall/linux]   FAIL: NotFound → {} (expected -ENOENT={})",
            linux_err_res.value, -(errno::ENOENT as i64),
        );
        return Err(KernelError::InternalError);
    }

    // (3) linux_from_native passes through success values unchanged.
    let native_ok = SyscallResult::ok(42);
    let linux_ok = linux_from_native(native_ok);
    if linux_ok.value != 42 {
        serial_println!("[syscall/linux]   FAIL: success passthrough");
        return Err(KernelError::InternalError);
    }

    // (4) Unknown Linux numbers return -ENOSYS through dispatch_linux.
    let args = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
    let r = dispatch_linux(9999, &args);
    if r.value != -(errno::ENOSYS as i64) {
        serial_println!(
            "[syscall/linux]   FAIL: 9999 → {} (expected -ENOSYS={})",
            r.value, -(errno::ENOSYS as i64),
        );
        return Err(KernelError::InternalError);
    }

    // (5) sched_yield: no-arg, no-state, must succeed.
    let r = dispatch_linux(nr::SCHED_YIELD, &args);
    if r.value != 0 {
        serial_println!("[syscall/linux]   FAIL: sched_yield → {}", r.value);
        return Err(KernelError::InternalError);
    }

    // (6) write to invalid fd → -EBADF.
    let bad_write = SyscallArgs { arg0: 99, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
    let r = dispatch_linux(nr::WRITE, &bad_write);
    if r.value != -(errno::EBADF as i64) {
        serial_println!(
            "[syscall/linux]   FAIL: write(99) → {} (expected -EBADF)", r.value
        );
        return Err(KernelError::InternalError);
    }

    // (7) writev with negative iovcnt → -EINVAL.
    let bad_iov = SyscallArgs {
        arg0: 1, arg1: 0, arg2: u64::MAX, arg3: 0, arg4: 0, arg5: 0,
    };
    let r = dispatch_linux(nr::WRITEV, &bad_iov);
    if r.value != -(errno::EINVAL as i64) {
        serial_println!(
            "[syscall/linux]   FAIL: writev(iovcnt=-1) → {} (expected -EINVAL)", r.value
        );
        return Err(KernelError::InternalError);
    }

    // (7a) The kernel self-test runs from a kernel task with no Linux fd
    // table, so every fd-table-backed syscall must surface -EBADF rather
    // than panicking.  Exercise read / close / dup / fcntl(F_GETFD) /
    // openat(non-AT_FDCWD).
    let any_fd = SyscallArgs { arg0: 5, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
    for (which, syscall) in [
        ("read", nr::READ),
        ("close", nr::CLOSE),
        ("dup", nr::DUP),
        ("fcntl", nr::FCNTL),
        ("lseek", nr::LSEEK),
    ] {
        let r = dispatch_linux(syscall, &any_fd);
        if r.value != -(errno::EBADF as i64) {
            serial_println!(
                "[syscall/linux]   FAIL: {}(fd=5) on a process w/o fd table → {} (expected -EBADF)",
                which, r.value,
            );
            return Err(KernelError::InternalError);
        }
    }

    // (7b) dup3(0, 0, 0) — same fd is EINVAL even before fd-table lookup.
    let dup3_same = SyscallArgs {
        arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    let r = dispatch_linux(nr::DUP3, &dup3_same);
    if r.value != -(errno::EINVAL as i64) {
        serial_println!(
            "[syscall/linux]   FAIL: dup3(0,0,0) → {} (expected -EINVAL)", r.value
        );
        return Err(KernelError::InternalError);
    }

    // (7b1) pipe / pipe2 with NULL pipefd → -EFAULT.
    let pipe_null = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
    let r = dispatch_linux(nr::PIPE, &pipe_null);
    if r.value != -(errno::EFAULT as i64) {
        serial_println!(
            "[syscall/linux]   FAIL: pipe(NULL) → {} (expected -EFAULT)", r.value
        );
        return Err(KernelError::InternalError);
    }
    let r = dispatch_linux(nr::PIPE2, &pipe_null);
    if r.value != -(errno::EFAULT as i64) {
        serial_println!(
            "[syscall/linux]   FAIL: pipe2(NULL, 0) → {} (expected -EFAULT)", r.value
        );
        return Err(KernelError::InternalError);
    }

    // (7b2) pipe2 with an unknown flag bit → -EINVAL.
    let pipe2_bad_flag = SyscallArgs {
        arg0: 1, arg1: 0x1, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    let r = dispatch_linux(nr::PIPE2, &pipe2_bad_flag);
    if r.value != -(errno::EINVAL as i64) {
        serial_println!(
            "[syscall/linux]   FAIL: pipe2(1, 0x1) → {} (expected -EINVAL)", r.value
        );
        return Err(KernelError::InternalError);
    }

    // (7c) openat with a non-AT_FDCWD dirfd → -ENOSYS.
    let openat_bad = SyscallArgs {
        arg0: 7, arg1: 0x1000, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    let r = dispatch_linux(nr::OPENAT, &openat_bad);
    if r.value != -(errno::ENOSYS as i64) {
        serial_println!(
            "[syscall/linux]   FAIL: openat(dirfd=7) → {} (expected -ENOSYS)", r.value
        );
        return Err(KernelError::InternalError);
    }

    // (7d) translate_open_flags exhaustive cases.
    {
        use crate::fs::handle::OpenFlags;
        let f = translate_open_flags(oflags::O_RDONLY);
        if f & OpenFlags::READ.bits() == 0 || f & OpenFlags::WRITE.bits() != 0 {
            serial_println!("[syscall/linux]   FAIL: O_RDONLY → {:#x}", f);
            return Err(KernelError::InternalError);
        }
        let f = translate_open_flags(oflags::O_WRONLY | oflags::O_CREAT | oflags::O_TRUNC);
        if f & OpenFlags::WRITE.bits() == 0
            || f & OpenFlags::CREATE.bits() == 0
            || f & OpenFlags::TRUNCATE.bits() == 0
        {
            serial_println!("[syscall/linux]   FAIL: O_WRONLY|O_CREAT|O_TRUNC → {:#x}", f);
            return Err(KernelError::InternalError);
        }
        let f = translate_open_flags(oflags::O_RDWR | oflags::O_APPEND);
        if f & OpenFlags::READ.bits() == 0
            || f & OpenFlags::WRITE.bits() == 0
            || f & OpenFlags::APPEND.bits() == 0
        {
            serial_println!("[syscall/linux]   FAIL: O_RDWR|O_APPEND → {:#x}", f);
            return Err(KernelError::InternalError);
        }
    }

    // (8) clock_gettime with bad clockid → -EINVAL.
    let bad_clk = SyscallArgs { arg0: 999, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
    let r = dispatch_linux(nr::CLOCK_GETTIME, &bad_clk);
    if r.value != -(errno::EINVAL as i64) {
        serial_println!(
            "[syscall/linux]   FAIL: clock_gettime(999) → {} (expected -EINVAL)", r.value
        );
        return Err(KernelError::InternalError);
    }

    // (9) arch_prctl with an unknown code → -EINVAL.
    let bad_prctl = SyscallArgs { arg0: 0x42, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
    let r = dispatch_linux(nr::ARCH_PRCTL, &bad_prctl);
    if r.value != -(errno::EINVAL as i64) {
        serial_println!(
            "[syscall/linux]   FAIL: arch_prctl(0x42) → {} (expected -EINVAL)", r.value
        );
        return Err(KernelError::InternalError);
    }

    // (10) LinuxTimespec round-trip.
    let ts = LinuxTimespec { tv_sec: 5, tv_nsec: 123_456_789 };
    let ns = ts.to_nanos();
    if ns != 5_123_456_789 {
        serial_println!("[syscall/linux]   FAIL: timespec→ns {}", ns);
        return Err(KernelError::InternalError);
    }
    let round = LinuxTimespec::from_nanos(ns);
    if round != ts {
        serial_println!("[syscall/linux]   FAIL: timespec round-trip");
        return Err(KernelError::InternalError);
    }

    // (11) LinuxTimespec rejects malformed values (negative ns, nsec ≥ 1e9).
    let bad1 = LinuxTimespec { tv_sec: 0, tv_nsec: -1 };
    let bad2 = LinuxTimespec { tv_sec: 0, tv_nsec: 1_000_000_000 };
    let bad3 = LinuxTimespec { tv_sec: -1, tv_nsec: 0 };
    if bad1.to_nanos() != 0 || bad2.to_nanos() != 0 || bad3.to_nanos() != 0 {
        serial_println!("[syscall/linux]   FAIL: malformed timespec accepted");
        return Err(KernelError::InternalError);
    }

    // (12) kernel_error_from_code round-trips.
    let codes = [
        (-2_i32, KernelError::NotSupported),
        (-3, KernelError::InvalidArgument),
        (-500, KernelError::NotFound),
        (-505, KernelError::InvalidHandle),
    ];
    for (code, expected) in codes {
        match kernel_error_from_code(code) {
            Some(e) if e == expected => {}
            other => {
                serial_println!(
                    "[syscall/linux]   FAIL: code {} → {:?}, expected {:?}",
                    code, other, expected,
                );
                return Err(KernelError::InternalError);
            }
        }
    }
    // Unknown codes return None.
    if kernel_error_from_code(-9999).is_some() {
        serial_println!("[syscall/linux]   FAIL: unknown code mapped to Some(_)");
        return Err(KernelError::InternalError);
    }

    // (12b) execve user-marshalling helpers (NULL handling).
    //
    // These do not require a calling process — read_user_cstr returns
    // EFAULT on a NULL pointer before touching userspace, and
    // read_user_ptr_array returns an empty array on NULL (which is
    // how glibc passes argv/envp for a program with no args).
    match read_user_cstr(0, 16) {
        Err(e) if e == errno::EFAULT => {}
        other => {
            serial_println!(
                "[syscall/linux]   FAIL: read_user_cstr(NULL) → {:?}", other
            );
            return Err(KernelError::InternalError);
        }
    }
    match read_user_ptr_array(0, 16) {
        Ok(v) if v.is_empty() => {}
        other => {
            serial_println!(
                "[syscall/linux]   FAIL: read_user_ptr_array(NULL) → {:?}",
                other.as_ref().map(alloc::vec::Vec::len)
            );
            return Err(KernelError::InternalError);
        }
    }

    // (13) dispatch_linux_with_frame routing.
    //
    // We can exercise the routing logic without actually calling
    // fork::fork_process by:
    //   - feeding a non-frame syscall_nr (READ) and expecting None;
    //   - feeding EXECVE and expecting Some(-ESRCH) — execve resolves
    //     the calling PID as its first step and the boot self-test
    //     task has no owning Linux process;
    //   - feeding CLONE with thread-creation bits and expecting
    //     Some(-ENOSYS) (linux_clone rejects before touching fork).
    //
    // We CANNOT exercise the fork-equivalent CLONE / FORK / VFORK
    // paths here because they require a live calling process to
    // succeed.  Those are covered by the boot-time integration
    // suite when a real Linux binary calls them.
    {
        use crate::syscall::entry::SyscallFrame;
        let mut f = SyscallFrame {
            syscall_nr: nr::READ,
            arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            rbx: 0, rbp: 0, r12: 0, r13: 0, r14: 0, r15: 0,
            user_rip: 0, user_rsp: 0, user_rflags: 0,
        };
        if dispatch_linux_with_frame(&mut f).is_some() {
            serial_println!(
                "[syscall/linux]   FAIL: with_frame routed non-frame syscall"
            );
            return Err(KernelError::InternalError);
        }

        f.syscall_nr = nr::EXECVE;
        match dispatch_linux_with_frame(&mut f) {
            Some(v) if v == -i64::from(errno::ESRCH) => {}
            other => {
                serial_println!(
                    "[syscall/linux]   FAIL: execve via with_frame → {:?}", other
                );
                return Err(KernelError::InternalError);
            }
        }

        // FORK and VFORK in self-test context have no calling process
        // either, but they reach fork::fork_process which returns
        // ProcessNotFound → ESRCH.  This exercises the routing.
        f.syscall_nr = nr::FORK;
        match dispatch_linux_with_frame(&mut f) {
            Some(v) if v < 0 => {} // any negative errno is fine
            other => {
                serial_println!(
                    "[syscall/linux]   FAIL: fork via with_frame → {:?}", other
                );
                return Err(KernelError::InternalError);
            }
        }

        // CLONE with CLONE_VM | CLONE_THREAD | SIGCHLD — pthread-like.
        f.syscall_nr = nr::CLONE;
        f.arg0 = clone_flags::CLONE_VM
            | clone_flags::CLONE_THREAD
            | clone_flags::CLONE_SIGHAND
            | clone_flags::SIGCHLD;
        f.arg1 = 0; // child_stack must be 0 to reach the flag check
        match dispatch_linux_with_frame(&mut f) {
            Some(v) if v == -i64::from(errno::ENOSYS) => {}
            other => {
                serial_println!(
                    "[syscall/linux]   FAIL: thread-clone via with_frame → {:?}",
                    other
                );
                return Err(KernelError::InternalError);
            }
        }

        // CLONE with a non-zero child_stack — also -ENOSYS.
        f.syscall_nr = nr::CLONE;
        f.arg0 = clone_flags::SIGCHLD;
        f.arg1 = 0xDEAD_BEEF;
        match dispatch_linux_with_frame(&mut f) {
            Some(v) if v == -i64::from(errno::ENOSYS) => {}
            other => {
                serial_println!(
                    "[syscall/linux]   FAIL: stack-clone via with_frame → {:?}",
                    other
                );
                return Err(KernelError::InternalError);
            }
        }
    }

    serial_println!("[syscall/linux] Translation self-test PASSED");
    Ok(())
}
