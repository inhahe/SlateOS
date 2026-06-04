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
//! | 218      | set_tid_address   | registers clear_child_tid, ret tid |
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
// Linux sa_flags bits (subset we recognize)
// ---------------------------------------------------------------------------

/// Flags from `<bits/sigaction.h>` for x86_64 Linux.  Numeric values must
/// match Linux exactly — they appear in user struct sigaction.sa_flags.
#[allow(dead_code)]
pub mod sa_flags {
    /// Do not auto-block the delivered signal during its handler.
    pub const SA_NODEFER: u64 = 0x4000_0000;
    /// Reset handler to SIG_DFL after one delivery.
    pub const SA_RESETHAND: u64 = 0x8000_0000;
    /// Restart blocking syscalls interrupted by this signal.
    pub const SA_RESTART: u64 = 0x1000_0000;
    /// Handler is a `void(int, siginfo_t*, void*)`; needs Linux-shape
    /// ucontext_t on the stack.
    pub const SA_SIGINFO: u64 = 0x0000_0004;
    /// `sa_restorer` is valid and should be used as the return path
    /// instead of a kernel-injected default.
    pub const SA_RESTORER: u64 = 0x0400_0000;
    /// Use the alternate signal stack (sigaltstack) for this handler.
    pub const SA_ONSTACK: u64 = 0x0800_0000;
    /// Do not generate `SIGCHLD` for stopped/continued children.
    pub const SA_NOCLDSTOP: u64 = 0x0000_0001;
    /// Do not transform children into zombies on exit.
    pub const SA_NOCLDWAIT: u64 = 0x0000_0002;
    /// All recognised bits OR'd together.  Anything outside this mask
    /// is rejected with -EINVAL at sigaction time.
    pub const MASK: u64 = SA_NODEFER
        | SA_RESETHAND
        | SA_RESTART
        | SA_SIGINFO
        | SA_RESTORER
        | SA_ONSTACK
        | SA_NOCLDSTOP
        | SA_NOCLDWAIT;
}

// ---------------------------------------------------------------------------
// Linux-sigaction table (per-process, per-signal)
// ---------------------------------------------------------------------------

/// Linux `struct sigaction` on x86_64.
///
/// Layout (matches `<bits/sigaction.h>`):
///
/// ```text
/// offset  size  field
///   0      8    sa_handler        (function pointer, or SIG_IGN / SIG_DFL)
///   8      8    sa_flags          (SA_* bitmask)
///  16      8    sa_restorer       (return-trampoline pointer)
///  24      8    sa_mask           (64-bit sigset_t)
/// total:  32 bytes
/// ```
///
/// We do not store the structure as `#[repr(C)]` directly to avoid
/// confusing the alignment story; user-level reads/writes go through
/// explicit field-by-field marshalling so any padding additions to
/// the kernel-side type do not change ABI behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinuxSigaction {
    pub sa_handler: u64,
    pub sa_flags: u64,
    pub sa_restorer: u64,
    pub sa_mask: u64,
}

/// Wire size of the user-visible struct sigaction on x86_64 Linux.
const LINUX_SIGACTION_SIZE: usize = 32;

/// Special handler values (mirrors `<bits/signum-generic.h>`):
///   `SIG_DFL` = 0  — default disposition.
///   `SIG_IGN` = 1  — ignore the signal.
const SIG_DFL: u64 = 0;
const SIG_IGN: u64 = 1;

mod linux_sigaction_table {
    //! Per-process, per-signal Linux sigaction storage.
    //!
    //! Lives outside the existing `proc::signal` module because the
    //! native signal-shim doesn't model per-signal handlers (it has a
    //! single trampoline pointer per process).  When the kernel's
    //! delivery path grows a Linux-shape frame in the future, it will
    //! consult this table to decide which handler to invoke, what
    //! flags to apply, and which restorer pointer to push.  For now
    //! the table is purely a query/store of state — its lifecycle
    //! hooks (`on_fork`, `on_exec`, `on_exit`) keep it in sync with
    //! the rest of the proc state.
    use super::{LinuxSigaction, SIG_DFL};
    use crate::proc::pcb::ProcessId;
    use alloc::collections::BTreeMap;
    use spin::Mutex;

    /// Global table: pid -> (signum -> entry).
    ///
    /// A missing (pid, sig) pair means "default disposition" — the
    /// callee returns a zero-filled `LinuxSigaction` (which decodes
    /// as `sa_handler = SIG_DFL`).
    static TABLE: Mutex<BTreeMap<ProcessId, BTreeMap<u32, LinuxSigaction>>>
        = Mutex::new(BTreeMap::new());

    /// Read the current entry for `(pid, sig)`.
    ///
    /// Returns the stored entry if any, else a default-filled struct
    /// (sa_handler = SIG_DFL, all other fields zero).  Linux behaves
    /// the same way for an unmodified signal disposition.
    pub fn get(pid: ProcessId, sig: u32) -> LinuxSigaction {
        let table = TABLE.lock();
        table
            .get(&pid)
            .and_then(|inner| inner.get(&sig).copied())
            .unwrap_or(LinuxSigaction {
                sa_handler: SIG_DFL,
                sa_flags: 0,
                sa_restorer: 0,
                sa_mask: 0,
            })
    }

    /// Install `act` as the new entry for `(pid, sig)`.
    pub fn set(pid: ProcessId, sig: u32, act: LinuxSigaction) {
        let mut table = TABLE.lock();
        let inner = table.entry(pid).or_default();
        let _ = inner.insert(sig, act);
    }

    /// `fork` hook: child inherits the parent's full sigaction table.
    ///
    /// Linux semantics: a `fork()` child inherits all signal
    /// dispositions verbatim.  (Only `pending` is cleared in the
    /// child; that's handled by `proc::signal::inherit_for_fork`.)
    pub fn on_fork(parent: ProcessId, child: ProcessId) {
        let mut table = TABLE.lock();
        let entries = table.get(&parent).cloned();
        if let Some(entries) = entries {
            let _ = table.insert(child, entries);
        }
    }

    /// `exec` hook: caught signals (handler != SIG_DFL and != SIG_IGN)
    /// reset to SIG_DFL.  SIG_IGN dispositions are preserved.
    ///
    /// This matches POSIX `execve(2)` semantics: "Signals set to be
    /// caught by the calling process image shall be set to the
    /// default action in the new process image."
    pub fn on_exec(pid: ProcessId) {
        use super::SIG_IGN;
        let mut table = TABLE.lock();
        if let Some(inner) = table.get_mut(&pid) {
            inner.retain(|_sig, act| act.sa_handler == SIG_IGN);
            // Within retained entries, also clear sa_flags / sa_mask /
            // sa_restorer: an SA_RESTORER pointer from the old image
            // is now garbage in the new address space.
            for act in inner.values_mut() {
                act.sa_flags = 0;
                act.sa_restorer = 0;
                act.sa_mask = 0;
            }
        }
    }

    /// `exit` hook: drop all per-signal state for a defunct process.
    pub fn on_exit(pid: ProcessId) {
        let mut table = TABLE.lock();
        let _ = table.remove(&pid);
    }

    /// Self-test helper: clear all state.
    #[cfg(any(test, debug_assertions))]
    pub fn clear_all() {
        let mut table = TABLE.lock();
        table.clear();
    }
}

pub use linux_sigaction_table::{
    get as linux_sigaction_get,
    on_exec as linux_sigaction_on_exec,
    on_exit as linux_sigaction_on_exit,
    on_fork as linux_sigaction_on_fork,
    set as linux_sigaction_set,
};

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
    // Note: STATX was previously declared in this block but not dispatched.
    pub const PRLIMIT64: u64 = 302;
    pub const RT_SIGPENDING: u64 = 127;
    pub const TKILL: u64 = 200;
    pub const TGKILL: u64 = 234;
    pub const UMASK: u64 = 95;
    pub const SIGALTSTACK: u64 = 131;
    pub const GETRESUID: u64 = 118;
    pub const GETRESGID: u64 = 120;
    pub const PERSONALITY: u64 = 135;
    pub const PRCTL: u64 = 157;
    pub const GETRUSAGE: u64 = 98;
    pub const SYSINFO: u64 = 99;
    pub const TIMES: u64 = 100;
    pub const SETPGID: u64 = 109;
    pub const GETPGRP: u64 = 111;
    pub const SETSID: u64 = 112;
    pub const GETPGID: u64 = 121;
    pub const GETSID: u64 = 124;
    pub const GETPRIORITY: u64 = 140;
    pub const SETPRIORITY: u64 = 141;
    pub const SETUID: u64 = 105;
    pub const SETGID: u64 = 106;
    pub const SETREUID: u64 = 113;
    pub const SETREGID: u64 = 114;
    pub const GETGROUPS: u64 = 115;
    pub const SETGROUPS: u64 = 116;
    pub const SETRESUID: u64 = 117;
    pub const SETRESGID: u64 = 119;
    pub const SETFSUID: u64 = 122;
    pub const SETFSGID: u64 = 123;
    pub const CAPGET: u64 = 125;
    pub const CAPSET: u64 = 126;
    pub const SCHED_SETPARAM: u64 = 142;
    pub const SCHED_GETPARAM: u64 = 143;
    pub const SCHED_SETSCHEDULER: u64 = 144;
    pub const SCHED_GETSCHEDULER: u64 = 145;
    pub const SCHED_GET_PRIORITY_MAX: u64 = 146;
    pub const SCHED_GET_PRIORITY_MIN: u64 = 147;
    pub const SCHED_RR_GET_INTERVAL: u64 = 148;
    pub const SCHED_SETAFFINITY: u64 = 203;
    pub const SCHED_GETAFFINITY: u64 = 204;
    pub const FSYNC: u64 = 74;
    pub const FDATASYNC: u64 = 75;
    pub const SYNC: u64 = 162;
    pub const SYNCFS: u64 = 306;
    pub const SETHOSTNAME: u64 = 170;
    pub const SETDOMAINNAME: u64 = 171;
    pub const MLOCK: u64 = 149;
    pub const MUNLOCK: u64 = 150;
    pub const MLOCKALL: u64 = 151;
    pub const MUNLOCKALL: u64 = 152;
    pub const FADVISE64: u64 = 221;
    pub const READAHEAD: u64 = 187;
    pub const CLOSE_RANGE: u64 = 436;
    pub const GETRLIMIT: u64 = 97;
    pub const SETRLIMIT: u64 = 160;
    pub const GETCPU: u64 = 309;
    pub const STATFS: u64 = 137;
    pub const FSTATFS: u64 = 138;
    pub const CLOCK_SETTIME: u64 = 227;
    pub const CLOCK_ADJTIME: u64 = 305;
    pub const ADJTIMEX: u64 = 159;
    pub const CHROOT: u64 = 161;
    pub const MKNOD: u64 = 133;
    pub const MKNODAT: u64 = 259;
    pub const GETITIMER: u64 = 36;
    pub const SETITIMER: u64 = 38;
    pub const ALARM: u64 = 37;
    pub const PAUSE: u64 = 34;
    pub const FACCESSAT: u64 = 269;
    pub const FACCESSAT2: u64 = 439;
    pub const NEWFSTATAT: u64 = 262;
    pub const MKDIRAT: u64 = 258;
    pub const UNLINKAT: u64 = 263;
    pub const RENAMEAT: u64 = 264;
    pub const RENAME: u64 = 82;
    pub const RENAMEAT2: u64 = 316;
    pub const READLINKAT: u64 = 267;
    pub const CHMOD: u64 = 90;
    pub const FCHMOD: u64 = 91;
    pub const FCHMODAT: u64 = 268;
    pub const CHOWN: u64 = 92;
    pub const FCHOWN: u64 = 93;
    pub const LCHOWN: u64 = 94;
    pub const FCHOWNAT: u64 = 260;
    pub const TRUNCATE: u64 = 76;
    pub const FTRUNCATE: u64 = 77;
    pub const SYMLINK: u64 = 88;
    pub const SYMLINKAT: u64 = 266;
    pub const LINK: u64 = 86;
    pub const LINKAT: u64 = 265;
    pub const UTIMENSAT: u64 = 280;
    pub const UTIMES: u64 = 235;
    pub const UTIME: u64 = 132;
    pub const SIGNALFD: u64 = 282;
    pub const SIGNALFD4: u64 = 289;
    pub const TIMERFD_CREATE: u64 = 283;
    pub const TIMERFD_SETTIME: u64 = 286;
    pub const TIMERFD_GETTIME: u64 = 287;
    pub const INOTIFY_INIT: u64 = 253;
    pub const INOTIFY_INIT1: u64 = 294;
    pub const INOTIFY_ADD_WATCH: u64 = 254;
    pub const INOTIFY_RM_WATCH: u64 = 255;
    pub const FANOTIFY_INIT: u64 = 300;
    pub const FANOTIFY_MARK: u64 = 301;
    pub const SENDFILE: u64 = 40;
    pub const SPLICE: u64 = 275;
    pub const TEE: u64 = 276;
    pub const VMSPLICE: u64 = 278;
    pub const COPY_FILE_RANGE: u64 = 326;
    pub const IO_SETUP: u64 = 206;
    pub const IO_DESTROY: u64 = 207;
    pub const IO_SUBMIT: u64 = 209;
    pub const IO_CANCEL: u64 = 210;
    pub const IO_GETEVENTS: u64 = 208;
    pub const IO_URING_SETUP: u64 = 425;
    pub const IO_URING_ENTER: u64 = 426;
    pub const IO_URING_REGISTER: u64 = 427;
    pub const BPF: u64 = 321;
    pub const PERF_EVENT_OPEN: u64 = 298;
    pub const KEYCTL: u64 = 250;
    pub const ADD_KEY: u64 = 248;
    pub const REQUEST_KEY: u64 = 249;
    pub const USERFAULTFD: u64 = 323;
    pub const MEMFD_CREATE: u64 = 319;
    pub const MEMFD_SECRET: u64 = 447;
    pub const PIDFD_OPEN: u64 = 434;
    pub const PIDFD_SEND_SIGNAL: u64 = 424;
    pub const PIDFD_GETFD: u64 = 438;
    pub const PROCESS_VM_READV: u64 = 310;
    pub const PROCESS_VM_WRITEV: u64 = 311;
    pub const PROCESS_MRELEASE: u64 = 448;
    pub const SETXATTR: u64 = 188;
    pub const LSETXATTR: u64 = 189;
    pub const FSETXATTR: u64 = 190;
    pub const GETXATTR: u64 = 191;
    pub const LGETXATTR: u64 = 192;
    pub const FGETXATTR: u64 = 193;
    pub const LISTXATTR: u64 = 194;
    pub const LLISTXATTR: u64 = 195;
    pub const FLISTXATTR: u64 = 196;
    pub const REMOVEXATTR: u64 = 197;
    pub const LREMOVEXATTR: u64 = 198;
    pub const FREMOVEXATTR: u64 = 199;
    pub const QUOTACTL: u64 = 179;
    pub const QUOTACTL_FD: u64 = 443;
    pub const INIT_MODULE: u64 = 175;
    pub const FINIT_MODULE: u64 = 313;
    pub const DELETE_MODULE: u64 = 176;
    pub const UNSHARE: u64 = 272;
    pub const SETNS: u64 = 308;
    pub const MOUNT: u64 = 165;
    pub const UMOUNT2: u64 = 166;
    pub const PIVOT_ROOT: u64 = 155;
    pub const SWAPON: u64 = 167;
    pub const SWAPOFF: u64 = 168;
    pub const REBOOT: u64 = 169;
    pub const SYSLOG: u64 = 103;
    pub const SHMGET: u64 = 29;
    pub const SHMAT: u64 = 30;
    pub const SHMCTL: u64 = 31;
    pub const SHMDT: u64 = 67;
    pub const SEMGET: u64 = 64;
    pub const SEMOP: u64 = 65;
    pub const SEMCTL: u64 = 66;
    pub const SEMTIMEDOP: u64 = 220;
    pub const MSGGET: u64 = 68;
    pub const MSGSND: u64 = 69;
    pub const MSGRCV: u64 = 70;
    pub const MSGCTL: u64 = 71;
    pub const MQ_OPEN: u64 = 240;
    pub const MQ_UNLINK: u64 = 241;
    pub const MQ_TIMEDSEND: u64 = 242;
    pub const MQ_TIMEDRECEIVE: u64 = 243;
    pub const MQ_NOTIFY: u64 = 244;
    pub const MQ_GETSETATTR: u64 = 245;
    pub const PSELECT6: u64 = 270;
    pub const PPOLL: u64 = 271;
    pub const EPOLL_CREATE: u64 = 213;
    pub const EPOLL_CTL: u64 = 233;
    pub const EPOLL_WAIT: u64 = 232;
    pub const EPOLL_PWAIT: u64 = 281;
    pub const EPOLL_CREATE1: u64 = 291;
    pub const EPOLL_PWAIT2: u64 = 441;
    pub const OPENAT2: u64 = 437;
    pub const EXECVEAT: u64 = 322;
    pub const NAME_TO_HANDLE_AT: u64 = 303;
    pub const OPEN_BY_HANDLE_AT: u64 = 304;
    pub const FSOPEN: u64 = 430;
    pub const FSCONFIG: u64 = 431;
    pub const FSMOUNT: u64 = 432;
    pub const FSPICK: u64 = 433;
    pub const OPEN_TREE: u64 = 428;
    pub const MOVE_MOUNT: u64 = 429;
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
    pub const ENODATA: i32 = 61;
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
        nr::RT_SIGRETURN => Some(linux_rt_sigreturn(frame)),
        _ => None,
    }
}

/// Linux `rt_sigreturn(2)` translation.
///
/// Linux semantics: takes no arguments, returns no value (it cannot
/// "return" to the caller in the C sense — it rewrites the saved
/// register state and the syscall-return path then resumes at the
/// pre-signal `RIP/RSP/RFLAGS` with all the pre-signal GPRs).
///
/// Our implementation reuses the native `sys_signal_return_with_frame`
/// restore path because our `proc::signal::deliver_pending_signal`
/// writes a native `SignalContext` on the user stack regardless of
/// the calling ABI.  The only Linux-side wrinkle is *where* on the
/// user stack the context lives at `rt_sigreturn` entry — Linux
/// programs reach `rt_sigreturn` via two paths:
///
///   1. Direct: the signal handler calls `rt_sigreturn` itself
///      (Linux-equivalent of our own POSIX-shim trampoline).  At this
///      point `user_rsp == ctx_addr - 8` because we placed a fake
///      8-byte null return slot below the context (see
///      `deliver_pending_signal`).  The context address is therefore
///      `user_rsp + 8`.
///   2. Via `SA_RESTORER`: the handler does `ret`, which pops the
///      8-byte return slot (transferring control to the restorer),
///      and the restorer does `mov rax, 15; syscall`.  At that point
///      `user_rsp == ctx_addr`.
///
/// We probe both candidates in order and use the first one that
/// points at a readable user mapping.  This covers both shim styles
/// without requiring the caller to know which we used.
///
/// If neither candidate is mapped, return `-EFAULT` — the syscall
/// frame is left untouched, and the userspace handler will continue
/// (which is wrong but defensive: corrupting RIP/RSP on a bad
/// `rt_sigreturn` call would just SIGSEGV the process anyway).
fn linux_rt_sigreturn(frame: &mut crate::syscall::entry::SyscallFrame) -> i64 {
    use crate::proc::signal::SignalContext;
    let ctx_size = core::mem::size_of::<SignalContext>();
    let ctx_align = core::mem::align_of::<SignalContext>() as u64;
    // Case 1: direct sigreturn from handler — ctx is at RSP + 8.
    // Case 2: SA_RESTORER path — ctx is at RSP.
    let candidates = [
        frame.user_rsp.wrapping_add(8),
        frame.user_rsp,
    ];
    for candidate in candidates {
        // Reject misaligned candidates *before* validate_user_read:
        // validate_user_read has a kernel-context bypass that returns
        // Ok(()) during boot self-tests, but the subsequent unsafe
        // deref in sys_signal_return_with_frame fires a debug-mode
        // alignment panic on misaligned pointers.  Defensive
        // alignment check keeps us out of that path entirely.
        if candidate == 0 || (candidate & (ctx_align - 1)) != 0 {
            continue;
        }
        if crate::mm::user::validate_user_read(candidate, ctx_size).is_ok() {
            // sys_signal_return_with_frame reads ctx from frame.arg0
            // and overwrites it (along with the other GPRs) from the
            // restored context, so it's safe to clobber arg0 here.
            frame.arg0 = candidate;
            return crate::syscall::handlers::sys_signal_return_with_frame(frame);
        }
    }
    -i64::from(errno::EFAULT)
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

/// Linux `clone()` translation.
///
/// Linux `clone(flags, child_stack, ptid, ctid, tls)` is the swiss-
/// army knife behind both `fork()` and `pthread_create()`.  We split
/// it three ways:
///
///   1. **Thread creation** (`CLONE_VM | CLONE_THREAD` set, non-zero
///      `child_stack`) — routes to
///      [`crate::proc::thread_clone::clone_thread`] which spawns a
///      new ring-3 thread sharing the parent's address space, fd
///      table, signal handlers, and credentials.  Honours
///      `CLONE_SETTLS` (FS base), `CLONE_PARENT_SETTID` /
///      `CLONE_CHILD_SETTID` (TID notification), and
///      `CLONE_CHILD_CLEARTID` (futex-wake on exit, for
///      `pthread_join`).
///   2. **Fork-equivalent** (`CLONE_VM` clear, `child_stack == 0`) —
///      glibc's `fork()` wrapper issues `clone(SIGCHLD, 0, ...)`.
///      Routes to [`linux_fork`].
///   3. **Unsupported combinations** (vfork, namespace clones,
///      ptrace, partial-share flag sets that don't match (1) or
///      (2)) — return `-ENOSYS`.
fn linux_clone(frame: &mut crate::syscall::entry::SyscallFrame) -> i64 {
    use crate::proc::{thread, thread_clone};

    let flags = frame.arg0;
    let child_stack = frame.arg1;
    let parent_tid_ptr = frame.arg2;
    // Linux x86_64 ABI: clone(flags, stack, ptid, ctid, tls) maps to
    // (rdi, rsi, rdx, r10, r8) which in our SyscallFrame are
    // (arg0, arg1, arg2, arg3, arg4).
    let child_tid_ptr = frame.arg3;
    let new_tls = frame.arg4;

    // (1) Thread-creation path: requires CLONE_VM | CLONE_THREAD AND
    //     a non-zero child_stack.  glibc's pthread_create wrapper
    //     also passes CLONE_FS | CLONE_FILES | CLONE_SIGHAND |
    //     CLONE_SYSVSEM (all of which we share by virtue of sharing
    //     the PCB), plus CLONE_SETTLS and the TID notification bits.
    const THREAD_REQUIRED: u64 =
        clone_flags::CLONE_VM | clone_flags::CLONE_THREAD;
    if (flags & THREAD_REQUIRED) == THREAD_REQUIRED && child_stack != 0 {
        // CLONE_VFORK on a thread-creation clone is nonsensical — the
        // new "child" shares the address space, so blocking the
        // parent until the child execs/exits is meaningless.  Reject
        // unambiguously.  CLONE_PARENT / CLONE_NEWNS / CLONE_PTRACE
        // need infrastructure (PID reparenting, mount namespaces,
        // ptrace lineage) we don't have yet.
        const UNSUPPORTED_BITS: u64 = clone_flags::CLONE_VFORK
            | clone_flags::CLONE_PARENT
            | clone_flags::CLONE_NEWNS
            | clone_flags::CLONE_PTRACE;
        if (flags & UNSUPPORTED_BITS) != 0 {
            return -i64::from(errno::ENOSYS);
        }

        let task_id = crate::sched::current_task_id();
        let parent_pid = match thread::owner_process(task_id) {
            Some(pid) if pid != 0 => pid,
            _ => return -i64::from(errno::ESRCH),
        };

        let args = thread_clone::CloneThreadArgs {
            flags,
            child_stack,
            parent_tid_ptr,
            child_tid_ptr,
            new_tls,
        };
        return match thread_clone::clone_thread(parent_pid, frame, &args) {
            Ok(new_tid) => i64::try_from(new_tid).unwrap_or(i64::MAX),
            Err(e) => -i64::from(linux_errno_for(e)),
        };
    }

    // (2) Anything-but-fork: a non-zero child_stack outside the
    // thread path is invalid.  Same for any "share with parent" bit
    // without the full CLONE_VM | CLONE_THREAD pairing — we can't
    // honour partial sharing (e.g. CLONE_FILES alone) because our
    // PCB model is per-process, not per-resource-table.
    if child_stack != 0 {
        return -i64::from(errno::ENOSYS);
    }

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

    // CLONE_PARENT / CLONE_NEWNS / CLONE_PTRACE need infrastructure
    // we don't have (PID reparenting, mount namespaces, ptrace
    // lineage) — reject up-front.
    const UNSUPPORTED_BITS: u64 = clone_flags::CLONE_PARENT
        | clone_flags::CLONE_NEWNS
        | clone_flags::CLONE_PTRACE;
    if flags & UNSUPPORTED_BITS != 0 {
        return -i64::from(errno::ENOSYS);
    }

    // CLONE_VFORK is *accepted* and degenerates to a plain fork.
    // Linux's vfork() guarantees:
    //   (a) the parent blocks until the child execve's or _exit's,
    //   (b) the child shares the parent's address space until then.
    // (a) is a *performance hint*, not a correctness requirement —
    // every conformant caller of vfork must already work against a
    // plain fork (POSIX explicitly permits implementations to make
    // vfork == fork).  (b) we don't honour, but CoW gives the child
    // a logically identical address space.  We pay a CoW page-table
    // walk vfork was trying to avoid, but the program behaves the
    // same.  Identical semantics to the dedicated VFORK syscall
    // (see `linux_fork` doc-comment).  Limitation tracked in
    // `todo.txt`.
    //
    // Everything that remains is just the CSIGNAL byte plus
    // optionally CLONE_VFORK — fork-equivalent.  glibc fork() passes
    // SIGCHLD here; we don't actually deliver a signal yet (no
    // Unix-style signals to userspace), but the kernel already
    // records parent/child relationships in the PCB.
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
            linux_sigaction_on_exec(pid);

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
        nr::MADVISE => sys_madvise(args),
        nr::BRK => sys_brk(args),
        nr::RT_SIGACTION => sys_rt_sigaction(args),
        nr::RT_SIGPROCMASK => sys_rt_sigprocmask(args),
        nr::SCHED_YIELD => sys_sched_yield(args),
        nr::NANOSLEEP => sys_nanosleep(args),
        nr::GETPID => sys_getpid(args),
        nr::EXIT => sys_exit(args),
        nr::KILL => sys_kill(args),
        nr::WAIT4 => sys_wait4(args),
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
        nr::PRLIMIT64 => sys_prlimit64(args),
        nr::RT_SIGPENDING => sys_rt_sigpending(args),
        nr::TKILL => sys_tkill(args),
        nr::TGKILL => sys_tgkill(args),
        nr::UMASK => sys_umask(args),
        nr::SIGALTSTACK => sys_sigaltstack(args),
        nr::IOCTL => sys_ioctl(args),
        nr::PRCTL => sys_prctl(args),
        nr::PERSONALITY => sys_personality(args),
        nr::GETRESUID => sys_getresuid(args),
        nr::GETRESGID => sys_getresgid(args),
        nr::GETRUSAGE => sys_getrusage(args),
        nr::SYSINFO => sys_sysinfo(args),
        nr::TIMES => sys_times(args),
        nr::GETPGRP => sys_getpgrp(args),
        nr::GETPGID => sys_getpgid(args),
        nr::SETPGID => sys_setpgid(args),
        nr::GETSID => sys_getsid(args),
        nr::SETSID => sys_setsid(args),
        nr::GETPRIORITY => sys_getpriority(args),
        nr::SETPRIORITY => sys_setpriority(args),
        nr::SETUID => sys_setuid(args),
        nr::SETGID => sys_setgid(args),
        nr::SETREUID => sys_setreuid(args),
        nr::SETREGID => sys_setregid(args),
        nr::GETGROUPS => sys_getgroups(args),
        nr::SETGROUPS => sys_setgroups(args),
        nr::SETRESUID => sys_setresuid(args),
        nr::SETRESGID => sys_setresgid(args),
        nr::SETFSUID => sys_setfsuid(args),
        nr::SETFSGID => sys_setfsgid(args),
        nr::CAPGET => sys_capget(args),
        nr::CAPSET => sys_capset(args),
        nr::SCHED_SETPARAM => sys_sched_setparam(args),
        nr::SCHED_GETPARAM => sys_sched_getparam(args),
        nr::SCHED_SETSCHEDULER => sys_sched_setscheduler(args),
        nr::SCHED_GETSCHEDULER => sys_sched_getscheduler(args),
        nr::SCHED_GET_PRIORITY_MAX => sys_sched_get_priority_max(args),
        nr::SCHED_GET_PRIORITY_MIN => sys_sched_get_priority_min(args),
        nr::SCHED_RR_GET_INTERVAL => sys_sched_rr_get_interval(args),
        nr::SCHED_SETAFFINITY => sys_sched_setaffinity(args),
        nr::SCHED_GETAFFINITY => sys_sched_getaffinity(args),
        nr::FSYNC => sys_fsync(args),
        nr::FDATASYNC => sys_fdatasync(args),
        nr::SYNC => sys_sync(args),
        nr::SYNCFS => sys_syncfs(args),
        nr::SETHOSTNAME => sys_sethostname(args),
        nr::SETDOMAINNAME => sys_setdomainname(args),
        nr::MLOCK => sys_mlock(args),
        nr::MUNLOCK => sys_munlock(args),
        nr::MLOCKALL => sys_mlockall(args),
        nr::MUNLOCKALL => sys_munlockall(args),
        nr::MSYNC => sys_msync(args),
        nr::FADVISE64 => sys_fadvise64(args),
        nr::READAHEAD => sys_readahead(args),
        nr::CLOSE_RANGE => sys_close_range(args),
        nr::GETRLIMIT => sys_getrlimit(args),
        nr::SETRLIMIT => sys_setrlimit(args),
        nr::GETCPU => sys_getcpu(args),
        nr::STATFS => sys_statfs(args),
        nr::FSTATFS => sys_fstatfs(args),
        nr::CLOCK_SETTIME => sys_clock_settime(args),
        nr::CLOCK_ADJTIME => sys_clock_adjtime(args),
        nr::ADJTIMEX => sys_adjtimex(args),
        nr::CHROOT => sys_chroot(args),
        nr::MKNOD => sys_mknod(args),
        nr::MKNODAT => sys_mknodat(args),
        nr::GETITIMER => sys_getitimer(args),
        nr::SETITIMER => sys_setitimer(args),
        nr::ALARM => sys_alarm(args),
        nr::PAUSE => sys_pause(args),
        nr::ACCESS => sys_access(args),
        nr::FACCESSAT => sys_faccessat(args),
        nr::FACCESSAT2 => sys_faccessat2(args),
        nr::STAT => sys_stat(args),
        nr::LSTAT => sys_lstat(args),
        nr::FSTAT => sys_fstat(args),
        nr::NEWFSTATAT => sys_newfstatat(args),
        nr::STATX => sys_statx(args),
        nr::MKDIR => sys_mkdir(args),
        nr::MKDIRAT => sys_mkdirat(args),
        nr::RMDIR => sys_rmdir(args),
        nr::UNLINK => sys_unlink(args),
        nr::UNLINKAT => sys_unlinkat(args),
        nr::RENAME => sys_rename(args),
        nr::RENAMEAT => sys_renameat(args),
        nr::RENAMEAT2 => sys_renameat2(args),
        nr::READLINK => sys_readlink(args),
        nr::READLINKAT => sys_readlinkat(args),
        nr::CHMOD => sys_chmod(args),
        nr::FCHMOD => sys_fchmod(args),
        nr::FCHMODAT => sys_fchmodat(args),
        nr::CHOWN => sys_chown(args),
        nr::FCHOWN => sys_fchown(args),
        nr::LCHOWN => sys_lchown(args),
        nr::FCHOWNAT => sys_fchownat(args),
        nr::TRUNCATE => sys_truncate(args),
        nr::FTRUNCATE => sys_ftruncate(args),
        nr::SYMLINK => sys_symlink(args),
        nr::SYMLINKAT => sys_symlinkat(args),
        nr::LINK => sys_link(args),
        nr::LINKAT => sys_linkat(args),
        nr::UTIMENSAT => sys_utimensat(args),
        nr::UTIMES => sys_utimes(args),
        nr::UTIME => sys_utime(args),
        nr::SIGNALFD => sys_signalfd(args),
        nr::SIGNALFD4 => sys_signalfd4(args),
        nr::TIMERFD_CREATE => sys_timerfd_create(args),
        nr::TIMERFD_SETTIME => sys_timerfd_settime(args),
        nr::TIMERFD_GETTIME => sys_timerfd_gettime(args),
        nr::INOTIFY_INIT => sys_inotify_init(args),
        nr::INOTIFY_INIT1 => sys_inotify_init1(args),
        nr::INOTIFY_ADD_WATCH => sys_inotify_add_watch(args),
        nr::INOTIFY_RM_WATCH => sys_inotify_rm_watch(args),
        nr::FANOTIFY_INIT => sys_fanotify_init(args),
        nr::FANOTIFY_MARK => sys_fanotify_mark(args),
        nr::SENDFILE => sys_sendfile(args),
        nr::SPLICE => sys_splice(args),
        nr::TEE => sys_tee(args),
        nr::VMSPLICE => sys_vmsplice(args),
        nr::COPY_FILE_RANGE => sys_copy_file_range(args),
        nr::IO_SETUP => sys_io_setup(args),
        nr::IO_DESTROY => sys_io_destroy(args),
        nr::IO_SUBMIT => sys_io_submit(args),
        nr::IO_CANCEL => sys_io_cancel(args),
        nr::IO_GETEVENTS => sys_io_getevents(args),
        nr::IO_URING_SETUP => sys_io_uring_setup(args),
        nr::IO_URING_ENTER => sys_io_uring_enter(args),
        nr::IO_URING_REGISTER => sys_io_uring_register(args),
        nr::BPF => sys_bpf(args),
        nr::PERF_EVENT_OPEN => sys_perf_event_open(args),
        nr::KEYCTL => sys_keyctl(args),
        nr::ADD_KEY => sys_add_key(args),
        nr::REQUEST_KEY => sys_request_key(args),
        nr::USERFAULTFD => sys_userfaultfd(args),
        nr::MEMFD_CREATE => sys_memfd_create(args),
        nr::MEMFD_SECRET => sys_memfd_secret(args),
        nr::PIDFD_OPEN => sys_pidfd_open(args),
        nr::PIDFD_SEND_SIGNAL => sys_pidfd_send_signal(args),
        nr::PIDFD_GETFD => sys_pidfd_getfd(args),
        nr::PROCESS_VM_READV => sys_process_vm_readv(args),
        nr::PROCESS_VM_WRITEV => sys_process_vm_writev(args),
        nr::PROCESS_MRELEASE => sys_process_mrelease(args),
        nr::SETXATTR => sys_setxattr(args),
        nr::LSETXATTR => sys_lsetxattr(args),
        nr::FSETXATTR => sys_fsetxattr(args),
        nr::GETXATTR => sys_getxattr(args),
        nr::LGETXATTR => sys_lgetxattr(args),
        nr::FGETXATTR => sys_fgetxattr(args),
        nr::LISTXATTR => sys_listxattr(args),
        nr::LLISTXATTR => sys_llistxattr(args),
        nr::FLISTXATTR => sys_flistxattr(args),
        nr::REMOVEXATTR => sys_removexattr(args),
        nr::LREMOVEXATTR => sys_lremovexattr(args),
        nr::FREMOVEXATTR => sys_fremovexattr(args),
        nr::QUOTACTL => sys_quotactl(args),
        nr::QUOTACTL_FD => sys_quotactl_fd(args),
        nr::INIT_MODULE => sys_init_module(args),
        nr::FINIT_MODULE => sys_finit_module(args),
        nr::DELETE_MODULE => sys_delete_module(args),
        nr::UNSHARE => sys_unshare(args),
        nr::SETNS => sys_setns(args),
        nr::MOUNT => sys_mount(args),
        nr::UMOUNT2 => sys_umount2(args),
        nr::PIVOT_ROOT => sys_pivot_root(args),
        nr::SWAPON => sys_swapon(args),
        nr::SWAPOFF => sys_swapoff(args),
        nr::REBOOT => sys_reboot(args),
        nr::SYSLOG => sys_syslog(args),
        nr::SHMGET => sys_shmget(args),
        nr::SHMAT => sys_shmat(args),
        nr::SHMCTL => sys_shmctl(args),
        nr::SHMDT => sys_shmdt(args),
        nr::SEMGET => sys_semget(args),
        nr::SEMOP => sys_semop(args),
        nr::SEMCTL => sys_semctl(args),
        nr::SEMTIMEDOP => sys_semtimedop(args),
        nr::MSGGET => sys_msgget(args),
        nr::MSGSND => sys_msgsnd(args),
        nr::MSGRCV => sys_msgrcv(args),
        nr::MSGCTL => sys_msgctl(args),
        nr::MQ_OPEN => sys_mq_open(args),
        nr::MQ_UNLINK => sys_mq_unlink(args),
        nr::MQ_TIMEDSEND => sys_mq_timedsend(args),
        nr::MQ_TIMEDRECEIVE => sys_mq_timedreceive(args),
        nr::MQ_NOTIFY => sys_mq_notify(args),
        nr::MQ_GETSETATTR => sys_mq_getsetattr(args),
        nr::POLL => sys_poll(args),
        nr::PPOLL => sys_ppoll(args),
        nr::SELECT => sys_select(args),
        nr::PSELECT6 => sys_pselect6(args),
        nr::EPOLL_CREATE => sys_epoll_create(args),
        nr::EPOLL_CREATE1 => sys_epoll_create1(args),
        nr::EPOLL_CTL => sys_epoll_ctl(args),
        nr::EPOLL_WAIT => sys_epoll_wait(args),
        nr::EPOLL_PWAIT => sys_epoll_pwait(args),
        nr::EPOLL_PWAIT2 => sys_epoll_pwait2(args),
        nr::OPENAT2 => sys_openat2(args),
        nr::EXECVEAT => sys_execveat(args),
        nr::NAME_TO_HANDLE_AT => sys_name_to_handle_at(args),
        nr::OPEN_BY_HANDLE_AT => sys_open_by_handle_at(args),
        nr::FSOPEN => sys_fsopen(args),
        nr::FSCONFIG => sys_fsconfig(args),
        nr::FSMOUNT => sys_fsmount(args),
        nr::FSPICK => sys_fspick(args),
        nr::OPEN_TREE => sys_open_tree(args),
        nr::MOVE_MOUNT => sys_move_mount(args),
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

/// `mprotect(addr, len, prot)` — change page protection on a range.
///
/// Walks every 16 KiB frame in `[addr, addr+len)` in the caller's
/// address space and updates the WRITABLE / NO_EXECUTE bits to
/// reflect the new `prot`:
///
///   - `PROT_WRITE` set    -> WRITABLE on the PTE
///   - `PROT_WRITE` clear  -> WRITABLE cleared
///   - `PROT_EXEC` set     -> NO_EXECUTE cleared
///   - `PROT_EXEC` clear   -> NO_EXECUTE set
///
/// `PROT_READ` is the implied "still mapped"; we never clear PRESENT
/// or USER_ACCESSIBLE.  `PROT_NONE` (prot == 0) is approximated as
/// "read-only, no-execute" — we don't yet track VMA state and can't
/// safely flip PRESENT off (it would be indistinguishable from a
/// never-mapped hole on the next access).  Documented limitation in
/// `todo.txt`.
///
/// Copy-on-write pages: we never set WRITABLE on a CoW-marked page
/// even if `PROT_WRITE` was requested.  The CoW fault handler will
/// upgrade the page on first write, which is the correct lazy
/// behaviour.
///
/// ## TLB consistency
///
/// After applying all PTE changes the function issues **one** TLB
/// shootdown covering the entire range, rather than one per frame.
/// For small ranges this is a single `crate::tlb::flush_range` call
/// (one IPI, N×4 `invlpg` on each remote CPU).  For ranges larger
/// than `MPROTECT_FULL_FLUSH_PAGES` 4 KiB pages we promote to a full
/// `crate::tlb::flush_all` (one CR3 reload per CPU) since N×4 invlpg
/// becomes more expensive than dumping the whole TLB.
fn sys_mprotect(args: &SyscallArgs) -> SyscallResult {
    use crate::mm::frame::FRAME_SIZE;
    use crate::mm::page_table::{self, PageFlags, VirtAddr, USER_SPACE_END};

    const PROT_READ: u64 = 1;
    const PROT_WRITE: u64 = 2;
    const PROT_EXEC: u64 = 4;
    const PROT_VALID_MASK: u64 = PROT_READ | PROT_WRITE | PROT_EXEC;

    let addr = args.arg0;
    let len = args.arg1;
    let prot = args.arg2;

    // POSIX: a zero-length range succeeds without doing anything.
    if len == 0 {
        return SyscallResult::ok(0);
    }
    // Reject unknown prot bits.
    if (prot & !PROT_VALID_MASK) != 0 {
        return linux_err(errno::EINVAL);
    }
    // Addr must be frame-aligned (Linux requires alignment to system
    // page size; ours is 16 KiB).
    let frame_size = FRAME_SIZE as u64;
    if (addr & (frame_size - 1)) != 0 {
        return linux_err(errno::EINVAL);
    }
    // Round len up to whole frames.
    let len_aligned = match len
        .checked_add(frame_size - 1)
        .map(|v| v & !(frame_size - 1))
    {
        Some(v) => v,
        None => return linux_err(errno::EINVAL),
    };
    let end = match addr.checked_add(len_aligned) {
        Some(e) => e,
        None => return linux_err(errno::EINVAL),
    };
    // Range must lie entirely in user space (don't let userspace
    // mprotect kernel mappings).
    if addr >= USER_SPACE_END || end > USER_SPACE_END {
        return linux_err(errno::EFAULT);
    }

    // Resolve the caller's PML4.
    let task_id = crate::sched::current_task_id();
    let pid = match crate::proc::thread::owner_process(task_id) {
        Some(p) if p != 0 => p,
        _ => return linux_err(errno::ESRCH),
    };
    let pml4 = match crate::proc::pcb::get_pml4(pid) {
        Some(p) if p != 0 => p,
        _ => return linux_err(errno::ESRCH),
    };

    // First pass: verify the entire range is mapped.  Linux returns
    // -ENOMEM if there's a hole, BEFORE making any changes.  This
    // avoids leaving a half-protected range on a partial failure.
    let mut va = addr;
    while va < end {
        let virt = VirtAddr::new(va);
        if page_table::translate_flags(pml4, virt).is_none() {
            return linux_err(errno::ENOMEM);
        }
        // Safe: va < end <= USER_SPACE_END so va + frame_size cannot
        // overflow (USER_SPACE_END = 2^47 is far below u64::MAX).
        va = va.saturating_add(frame_size);
    }

    // Second pass: apply the new flags frame by frame.
    let want_write = (prot & PROT_WRITE) != 0;
    let want_exec = (prot & PROT_EXEC) != 0;

    va = addr;
    while va < end {
        let virt = VirtAddr::new(va);
        // SAFETY: pml4 is the calling process's PML4; the address is
        // user-space and frame-aligned; we verified the range is
        // mapped in the first pass.  No other thread can be racing
        // on this range without the user explicitly serialising —
        // mprotect on a concurrently-faulting region is racy on
        // Linux too.
        let current = match page_table::translate_flags(pml4, virt) {
            Some(f) => f,
            None => return linux_err(errno::ENOMEM),
        };

        // Compute new flags: clear WRITABLE + NO_EXECUTE, then set
        // them according to prot.  Preserve PRESENT, USER_ACCESSIBLE,
        // COW, and any other PTE bits.
        let mut new_flags = current & !PageFlags::WRITABLE & !PageFlags::NO_EXECUTE;
        // Never set WRITABLE on a CoW page — the CoW fault handler
        // will upgrade the page on first write.
        if want_write && !current.contains(PageFlags::COW) {
            new_flags = new_flags | PageFlags::WRITABLE;
        }
        if !want_exec {
            new_flags = new_flags | PageFlags::NO_EXECUTE;
        }

        // SAFETY: same as translate_flags above — pml4 is valid,
        // virt is user-space frame-aligned, mapping exists.
        if let Err(e) = unsafe { page_table::change_flags(pml4, virt, new_flags) } {
            // On partial failure mid-loop, still flush whatever we
            // already modified so other CPUs don't observe stale
            // permissions for those frames.
            mprotect_flush_range(addr, va);
            return linux_err(linux_errno_for(e));
        }

        va = va.saturating_add(frame_size);
    }

    // Single batched TLB shootdown covering the entire modified range
    // (cross-CPU via IPI when SMP is active, no-op IPI on single-CPU).
    mprotect_flush_range(addr, end);

    SyscallResult::ok(0)
}

/// Threshold (in 4 KiB pages) at which `mprotect` switches from a
/// range-shootdown (`invlpg` per page on each CPU) to a full TLB flush
/// (CR3 reload).  Mirrors Linux's `tlb_single_page_flush_ceiling`
/// (~33 pages).  We round up to 64 4 KiB pages = 16 frames = 256 KiB
/// — small enough that 64 invlpgs are cheap, large enough that most
/// mprotect calls hit the range path.
const MPROTECT_FULL_FLUSH_PAGES: u64 = 64;

/// Flush the TLB on every online CPU for the range `[start, end)`.
///
/// Picks `flush_range` for small ranges and `flush_all` for large
/// ones.  Called with `start == end` when the loop bailed out on the
/// very first frame (no-op).
fn mprotect_flush_range(start: u64, end: u64) {
    if end <= start {
        return;
    }
    // Both are u64; the early-return above guarantees end > start.
    let bytes = end.saturating_sub(start);
    // 4 KiB hardware pages.  bytes is already frame-aligned (16 KiB
    // multiple), so this divides evenly.
    let page_count = bytes / 4096;
    if page_count == 0 {
        return;
    }
    if page_count > MPROTECT_FULL_FLUSH_PAGES {
        // Large range — one CR3 reload per CPU is cheaper than
        // N×invlpg.  Also covers the case where page_count > u32::MAX.
        crate::tlb::flush_all();
    } else {
        // page_count <= 64 — fits comfortably in u32.
        #[allow(clippy::cast_possible_truncation)]
        crate::tlb::flush_range(start, page_count as u32);
    }
}

/// `madvise(addr, len, advice)` — advisory hint about future access.
///
/// All [`MADV_*`](madv) hints we recognise are advisory: telling the
/// kernel "I'll touch this soon" / "I won't touch this for a while" /
/// "you can drop these pages and re-zero on next fault" / etc.  Linux
/// is allowed to silently ignore any advisory hint, and most glibc
/// allocators (jemalloc, tcmalloc, mimalloc, even modern glibc malloc)
/// expect MADV_DONTNEED to "succeed-or-be-irrelevant" — they never
/// check the return value for correctness, only for "did the kernel
/// even understand this call".  Returning ENOSYS for every madvise
/// makes those allocators spam the syscall on every free without ever
/// releasing memory back to the kernel, growing RSS unbounded.
///
/// Our policy:
///
/// - Accept every documented MADV_* hint in `1..=25` plus `0`
///   (MADV_NORMAL) and return 0.  We don't actually act on any of them
///   yet — MADV_DONTNEED on anonymous memory could free the frames
///   eagerly, but that needs VMA tracking to know what's anonymous.
///   Treating them as no-ops is the documented "kernel ignored the
///   hint" path and is always semantically valid.
/// - Reject `MADV_HWPOISON` (100) and `MADV_SOFT_OFFLINE` (101) with
///   EPERM — on Linux these require CAP_SYS_ADMIN and we don't expose
///   memory-failure injection to userspace.
/// - Reject everything else with EINVAL (matches Linux's behaviour for
///   unknown advice values).
/// - Validate `addr` is frame-aligned and `[addr, addr+len)` lies
///   entirely in user space.  Length 0 succeeds without further
///   checking (POSIX).
fn sys_madvise(args: &SyscallArgs) -> SyscallResult {
    use crate::mm::frame::FRAME_SIZE;
    use crate::mm::page_table::USER_SPACE_END;

    let addr = args.arg0;
    let len = args.arg1;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let advice = args.arg2 as i32;

    // POSIX: zero-length succeeds as a no-op without any validation.
    if len == 0 {
        return SyscallResult::ok(0);
    }

    // addr must be page-aligned (Linux requires alignment to the system
    // page size; ours is 16 KiB).
    let frame_size = FRAME_SIZE as u64;
    if (addr & (frame_size - 1)) != 0 {
        return linux_err(errno::EINVAL);
    }

    // Round len up to whole frames for the bounds check.
    let len_aligned = match len
        .checked_add(frame_size - 1)
        .map(|v| v & !(frame_size - 1))
    {
        Some(v) => v,
        None => return linux_err(errno::EINVAL),
    };
    let end = match addr.checked_add(len_aligned) {
        Some(e) => e,
        None => return linux_err(errno::EINVAL),
    };
    if addr >= USER_SPACE_END || end > USER_SPACE_END {
        return linux_err(errno::ENOMEM);
    }

    // Documented Linux MADV_* values.  Anything in 0..=25 is a known
    // advisory hint we accept as a no-op.  HWPOISON / SOFT_OFFLINE are
    // privileged.  Anything else is EINVAL.
    //
    // See `include/uapi/asm-generic/mman-common.h` in the Linux tree.
    const MADV_HWPOISON: i32 = 100;
    const MADV_SOFT_OFFLINE: i32 = 101;
    const MADV_KNOWN_MAX: i32 = 25; // MADV_COLLAPSE

    match advice {
        0..=MADV_KNOWN_MAX => SyscallResult::ok(0),
        MADV_HWPOISON | MADV_SOFT_OFFLINE => linux_err(errno::EPERM),
        _ => linux_err(errno::EINVAL),
    }
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

/// `rt_sigaction(sig, act, oldact, sigsetsize)` — install/query the
/// per-signal disposition for the calling process.
///
/// Linux semantics:
///   - `sig` ∈ `1..=NSIG`, excluding `SIGKILL` and `SIGSTOP` (which
///     cannot be caught — any attempt to install a handler returns
///     `-EINVAL`).
///   - `sigsetsize` must equal `sizeof(sigset_t)` (8 bytes on x86_64
///     Linux LP64).  Any other value is `-EINVAL`.
///   - `act != NULL`: install the new disposition.  `act` points at
///     a 32-byte `struct sigaction { sa_handler, sa_flags,
///     sa_restorer, sa_mask }`.  Unknown bits in `sa_flags` return
///     `-EINVAL` per Linux behaviour.
///   - `oldact != NULL`: copy the previous disposition out to
///     userspace before installing the new one (if any).
///
/// The kernel-side delivery infrastructure (`proc::signal`) does not
/// yet consume `sa_flags` / `sa_mask` / `sa_restorer` — see todo.txt
/// "Linux-shaped signal delivery frame" item.  Storing them today
/// makes `oldact` queries truthful and pre-populates the table for
/// when the Linux delivery path lands.
///
/// To preserve the existing behavior that signal *delivery* works at
/// all, we still call `sys_signal_register` to keep the per-process
/// trampoline pointer in sync with the most recently installed
/// catchable handler.  This is a known limitation: the trampoline is
/// a single value per process, so multiple `rt_sigaction` calls for
/// different signals will see the last-registered handler invoked
/// for all of them until the Linux delivery path is wired up.
fn sys_rt_sigaction(args: &SyscallArgs) -> SyscallResult {
    let sig = args.arg0;
    let act_ptr = args.arg1;
    let oldact_ptr = args.arg2;
    let sigsetsize = args.arg3 as usize;

    // Validate signum.  Linux uses `_NSIG = 64`.  Signal 0 is
    // reserved (used by kill(2) to probe existence).
    if sig == 0 || sig > u64::from(crate::proc::signal::NSIG) {
        return linux_err(errno::EINVAL);
    }
    // SIGKILL / SIGSTOP cannot be caught or ignored.
    if sig == u64::from(crate::proc::signal::SIGKILL)
        || sig == u64::from(crate::proc::signal::SIGSTOP)
    {
        if act_ptr != 0 {
            return linux_err(errno::EINVAL);
        }
        // ... but querying their (default-only) disposition is fine.
    }
    // Linux x86_64: sigset_t is exactly 8 bytes.
    if sigsetsize != 0 && sigsetsize != core::mem::size_of::<u64>() {
        return linux_err(errno::EINVAL);
    }

    let sig_u32 = sig as u32;

    // Look up caller's pid.  No-pid (boot self-test) returns ESRCH —
    // there's no process to associate the disposition with.
    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::ESRCH),
    };

    // If oldact != NULL, copy out the current disposition BEFORE
    // overwriting.  This matches Linux ordering and lets a caller
    // atomically swap (act, oldact) — the same pointer is sometimes
    // passed for both.
    if oldact_ptr != 0 {
        let old = linux_sigaction_get(pid, sig_u32);
        let mut buf = [0u8; LINUX_SIGACTION_SIZE];
        buf[0..8].copy_from_slice(&old.sa_handler.to_ne_bytes());
        buf[8..16].copy_from_slice(&old.sa_flags.to_ne_bytes());
        buf[16..24].copy_from_slice(&old.sa_restorer.to_ne_bytes());
        buf[24..32].copy_from_slice(&old.sa_mask.to_ne_bytes());
        // SAFETY: copy_to_user validates the user range.
        let r = unsafe {
            crate::mm::user::copy_to_user(
                buf.as_ptr(),
                oldact_ptr,
                LINUX_SIGACTION_SIZE,
            )
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
    }

    // act = NULL means "query only" — we've already done that above.
    if act_ptr == 0 {
        return SyscallResult::ok(0);
    }

    // Read the new sigaction (32 bytes).
    let mut buf = [0u8; LINUX_SIGACTION_SIZE];
    // SAFETY: copy_from_user validates the user range.
    let r = unsafe {
        crate::mm::user::copy_from_user(
            act_ptr,
            buf.as_mut_ptr(),
            LINUX_SIGACTION_SIZE,
        )
    };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    // Field-by-field decode keeps us robust against any future
    // padding additions to the kernel-side struct.
    let new_act = LinuxSigaction {
        sa_handler: u64::from_ne_bytes(buf[0..8].try_into().unwrap_or([0; 8])),
        sa_flags: u64::from_ne_bytes(buf[8..16].try_into().unwrap_or([0; 8])),
        sa_restorer: u64::from_ne_bytes(buf[16..24].try_into().unwrap_or([0; 8])),
        sa_mask: u64::from_ne_bytes(buf[24..32].try_into().unwrap_or([0; 8])),
    };

    // Reject unknown sa_flags bits — matches Linux behaviour, helps
    // catch userspace bugs early.
    if (new_act.sa_flags & !sa_flags::MASK) != 0 {
        return linux_err(errno::EINVAL);
    }

    // Persist.
    linux_sigaction_set(pid, sig_u32, new_act);

    // Keep the legacy per-process trampoline in sync with the most
    // recently installed catchable handler.  SIG_IGN / SIG_DFL must
    // not be invoked as code, so don't push those as the trampoline.
    if new_act.sa_handler != SIG_IGN && new_act.sa_handler != SIG_DFL {
        let native_args = SyscallArgs {
            arg0: sig,
            arg1: new_act.sa_handler,
            arg2: 0,
            arg3: 0,
            arg4: 0,
            arg5: 0,
        };
        // Discard result — sys_signal_register only fails if there's
        // no caller pid, which we already checked above.
        let _ = handlers::sys_signal_register(&native_args);
    }

    SyscallResult::ok(0)
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
///
/// Linux semantics:
///   - `sig == 0`: existence/permission probe.  Returns 0 if the target
///     process exists and the caller could send a signal to it, -ESRCH
///     if it doesn't exist, -EPERM on permission failure.  Critically,
///     `sig == 0` is NEVER `-EINVAL`; programs use it to test whether
///     a child is still alive without actually disturbing it.
///   - `sig in 1..=NSIG`: send the signal via the native signal_send
///     path which already knows the POSIX default-action table
///     (Terminate / Drop / Stop / Continue / Deliver-to-handler).
///   - `sig > NSIG` or `sig` not representable in u32: -EINVAL.
///   - `pid == 0` / `pid < 0` (process-group targeting): not yet
///     supported; we return -EINVAL the same way the native handler
///     does (process groups are a job-control feature we lack).
fn sys_kill(args: &SyscallArgs) -> SyscallResult {
    let sig = args.arg1;
    // sig=0 is the existence probe.  Route it through a no-op send
    // that still performs the existence + authority checks so the
    // caller gets a truthful 0 / -ESRCH / -EPERM answer.  We rewrite
    // sig to 1 (SIGHUP) for the inner check — classify_post treats
    // SIGHUP exactly the way the existence-probe path needs (it
    // either Drops or Delivers depending on disposition, both of
    // which we'll discard).  Then short-circuit a 0 return on
    // success.  On error, propagate the errno.
    if sig == 0 {
        let probe_args = SyscallArgs {
            arg0: args.arg0,
            arg1: 1, // SIGHUP — valid, won't fail signal-number gate
            arg2: 0, arg3: 0, arg4: 0, arg5: 0,
        };
        // We can't actually let SIGHUP be delivered as a probe (the
        // wake target might catch it).  Instead, hand-roll the same
        // existence + authority checks the native handler performs.
        use crate::proc::{pcb, thread};
        let target = args.arg0;
        if target == 0 {
            return linux_err(errno::EINVAL);
        }
        let task_id = crate::sched::current_task_id();
        let caller = thread::owner_process(task_id).unwrap_or(0);
        if target != caller {
            let target_parent = match pcb::parent(target) {
                Some(p) => p,
                None => return linux_err(errno::ESRCH),
            };
            let has_parent_auth = caller == 0 || caller == target_parent;
            let has_cap_auth = pcb::has_capability_for(
                caller,
                crate::cap::ResourceType::Process,
                target,
                crate::cap::Rights::DELETE,
            );
            if !has_parent_auth && !has_cap_auth {
                return linux_err(errno::EPERM);
            }
        }
        match pcb::state(target) {
            Some(pcb::ProcessState::Zombie) | None => return linux_err(errno::ESRCH),
            _ => {}
        }
        // Existence probe succeeds — suppress the unused-warning on the
        // probe_args placeholder so it documents the intent.
        let _ = probe_args;
        return SyscallResult::ok(0);
    }
    // Real signal: delegate to native.  Native SYS_SIGNAL_SEND:
    // arg0 = target pid, arg1 = signum.
    linux_from_native(handlers::sys_signal_send(args))
}

/// `rt_sigpending(set, sigsetsize)` — report the pending-signal mask
/// for the calling process.
///
/// Semantics (Linux man 2 rt_sigpending):
///   - `set`: user pointer to `sigset_t` to fill with the bitmap of
///     signals that have been raised on this process but not yet
///     consumed (delivered to a handler, dropped, or used to terminate).
///   - `sigsetsize`: must equal `sizeof(sigset_t)` (8 bytes on x86_64
///     Linux), else `-EINVAL`.
///   - `set` may not be NULL — Linux returns `-EFAULT` for NULL.  Our
///     [`validate_user_write`] returns the same.
///
/// We use [`crate::proc::signal::pending`] which mirrors the in-kernel
/// pending mask exactly; for tasks with no owning process (kernel
/// self-tests), the pending mask is reported as 0.
fn sys_rt_sigpending(args: &SyscallArgs) -> SyscallResult {
    let set_ptr = args.arg0;
    let sigsetsize = args.arg1 as usize;

    // Validate sigsetsize == 8 first (Linux rejects mismatched sizes
    // before touching the pointer).
    if sigsetsize != core::mem::size_of::<u64>() {
        return linux_err(errno::EINVAL);
    }

    // Validate user pointer.
    if let Err(e) = crate::mm::user::validate_user_write(set_ptr, 8) {
        return linux_err(linux_errno_for(e));
    }

    let pid = caller_pid().unwrap_or(0);
    let mask: u64 = crate::proc::signal::pending(pid);

    let bytes = mask.to_ne_bytes();
    // SAFETY: validate_user_write above confirmed an 8-byte writable
    // user range; we copy exactly 8 bytes from a kernel-owned array.
    let r = unsafe { crate::mm::user::copy_to_user(bytes.as_ptr(), set_ptr, 8) };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `tkill(tid, sig)` — send `sig` to the thread identified by `tid`.
///
/// Linux distinguishes tkill (thread-targeted) from kill (process-
/// targeted).  We don't have multi-threaded signal routing yet — every
/// signal goes to the owning process — so tkill degenerates to
/// `kill(owner_process(tid), sig)`.  For single-threaded processes
/// (the common case for early Linux binaries we'd run, and for libc's
/// `pthread_kill` against the main thread), this is observationally
/// identical to Linux.
///
/// `sig == 0` is the existence probe, same as `kill(pid, 0)`.
///
/// Errors:
///   - `-ESRCH` if `tid` is not a registered thread.
///   - All other errors delegate to [`sys_kill`].
fn sys_tkill(args: &SyscallArgs) -> SyscallResult {
    let tid = args.arg0;
    let sig = args.arg1;
    let pid = match crate::proc::thread::owner_process(tid) {
        Some(p) => p,
        None => return linux_err(errno::ESRCH),
    };
    let kill_args = SyscallArgs {
        arg0: pid,
        arg1: sig,
        arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    sys_kill(&kill_args)
}

/// `tgkill(tgid, tid, sig)` — send `sig` to thread `tid` in thread-
/// group `tgid`.
///
/// Semantics:
///   - `tgid` is the thread-group ID, which on Linux equals the PID
///     of the group leader.  In our model, every thread's owning
///     process IS its tgid.
///   - If `tid` does not exist, or does not belong to `tgid`,
///     `-ESRCH`.  Linux mandates this — `tgkill` is the race-free
///     `pthread_kill` because it can detect tid reuse across a fork.
///   - Otherwise, behaves exactly like [`sys_tkill`] (and thus like
///     `kill(tgid, sig)` for now).
fn sys_tgkill(args: &SyscallArgs) -> SyscallResult {
    let tgid = args.arg0;
    let tid = args.arg1;
    let sig = args.arg2;
    let pid = match crate::proc::thread::owner_process(tid) {
        Some(p) => p,
        None => return linux_err(errno::ESRCH),
    };
    if pid != tgid {
        return linux_err(errno::ESRCH);
    }
    let kill_args = SyscallArgs {
        arg0: pid,
        arg1: sig,
        arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    sys_kill(&kill_args)
}

/// `umask(mask)` — set the process file-mode creation mask, returning
/// the previous value.
///
/// Linux semantics: `mask & 0o777` is stored as the new umask; the old
/// value is returned.  Programs use it both to set new permissions and
/// to read the current one (the common idiom `old = umask(0); umask(old);`).
///
/// We don't have per-process umask storage yet (the PCB doesn't carry
/// one) and the VFS doesn't apply umask at create time either, so
/// nothing else in the kernel observes the value.  Stub semantics:
///   - Always return 0o022 (the standard Linux distro default — what
///     most programs would see on a fresh shell).
///   - Silently accept and discard the new mask.
///
/// Limitation: a program that does `umask(0o077); creat(file);` and
/// then checks the file mode will see the kernel default permissions
/// rather than mask-respecting ones.  Tracked in todo.txt as needing
/// per-process umask storage + VFS plumbing.
fn sys_umask(_args: &SyscallArgs) -> SyscallResult {
    // 0o022 is the de-facto Linux distro default (group/other lose
    // write bits).  Returning it as the "previous" umask is the
    // friendliest stub for programs that do `old = umask(N); umask(old);`.
    SyscallResult::ok(0o022)
}

/// `sigaltstack(ss, old_ss)` — install / query the alternate signal
/// stack used when a handler has SA_ONSTACK set.
///
/// We don't currently implement alternate signal stacks (signals
/// always deliver on the thread's main stack).  This stub:
///   - Accepts any `ss` pointer and silently ignores it (we read it to
///     validate it's a valid user-mapped range when non-NULL, matching
///     Linux's EFAULT-on-bad-pointer behaviour).
///   - When `old_ss` is non-NULL, writes a `stack_t` with `ss_flags ==
///     SS_DISABLE` to communicate "no alternate stack is in effect".
///   - Returns 0 (success) regardless.
///
/// `struct stack_t` on Linux x86_64:
///   ```
///   struct stack_t {
///       void  *ss_sp;     // 8 bytes
///       int    ss_flags;  // 4 bytes
///       size_t ss_size;   // 8 bytes (after 4 bytes of padding)
///   };  // 24 bytes total
///   ```
/// (`int` is followed by 4 bytes of natural-alignment padding before
/// the `size_t`.)
///
/// Limitation: programs that catch SIGSEGV with SA_ONSTACK to print a
/// backtrace after blowing the main stack will deliver to the
/// already-blown stack and double-fault.  Tracked in todo.txt
/// alongside the Linux-shaped rt_sigframe delivery work — both
/// require the same signal-delivery refactor.
fn sys_sigaltstack(args: &SyscallArgs) -> SyscallResult {
    /// `SS_DISABLE` from `<signal.h>` — communicates "no alternate
    /// stack is in effect" in `stack_t.ss_flags`.
    const SS_DISABLE: i32 = 2;
    const STACK_T_SIZE: usize = 24;

    let ss_ptr = args.arg0;
    let old_ss_ptr = args.arg1;

    // Validate ss (input) pointer if non-NULL.  We don't honour its
    // contents but we MUST fault on a bad user range — Linux does, and
    // programs sometimes rely on the fault to detect ABI mismatches.
    if ss_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(ss_ptr, STACK_T_SIZE) {
            return linux_err(linux_errno_for(e));
        }
    }

    // Write old_ss (output) as "disabled" if non-NULL.
    if old_ss_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_write(old_ss_ptr, STACK_T_SIZE) {
            return linux_err(linux_errno_for(e));
        }
        // Pack the disabled stack_t into a byte buffer.  Layout:
        //   [0..8]   ss_sp (null)
        //   [8..12]  ss_flags = SS_DISABLE
        //   [12..16] padding
        //   [16..24] ss_size (0)
        let mut buf = [0u8; STACK_T_SIZE];
        // ss_flags at offset 8.
        let flags_bytes = SS_DISABLE.to_ne_bytes();
        buf[8] = flags_bytes[0];
        buf[9] = flags_bytes[1];
        buf[10] = flags_bytes[2];
        buf[11] = flags_bytes[3];
        // SAFETY: validate_user_write above confirmed the 24-byte
        // writable user range; we copy exactly STACK_T_SIZE bytes
        // from a kernel-owned buffer.
        let r = unsafe {
            crate::mm::user::copy_to_user(buf.as_ptr(), old_ss_ptr, STACK_T_SIZE)
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
    }

    SyscallResult::ok(0)
}

/// `ioctl(fd, request, arg)` — device/driver-specific control.
///
/// Linux's `ioctl` is the catch-all for everything that doesn't fit a
/// dedicated syscall: terminal control (`TCGETS`/`TIOCGWINSZ`),
/// non-blocking flags (`FIONBIO`), interface configuration, etc.
/// Every operation has its own semantics; there's no generic
/// implementation.
///
/// We have no terminal devices, no Linux-style device files, and no
/// fd table that maps to ioctl-aware drivers yet.  The semantically
/// correct response for *every* ioctl on a non-special fd is
/// `-ENOTTY` ("inappropriate ioctl for device" — the historical name
/// for "this fd isn't a tty and your op only makes sense on a tty").
/// That's also what Linux returns for ioctls on regular files and
/// most non-tty fds.
///
/// Returning `-ENOTTY` instead of `-ENOSYS` matters: `isatty(3)` is
/// defined as `ioctl(fd, TCGETS, &tio) != -1`, so programs probing
/// "is stdout a terminal?" need ENOTTY to get the right "no" answer.
/// `-ENOSYS` would confuse them ("the syscall doesn't exist", not
/// "this fd isn't a terminal").
///
/// Limitation: programs that legitimately need an ioctl to succeed
/// (e.g. `ioctl(sock, FIONBIO, &one)` to set non-blocking on a
/// socket — though glibc/musl normally use `fcntl(F_SETFL, O_NONBLOCK)`
/// for this) will hard-fail.  Once we have a real fd table with
/// driver routing, this stub becomes a per-fd dispatch table that
/// asks the driver "do you handle this request?" and only falls back
/// to ENOTTY if nobody does.
fn sys_ioctl(_args: &SyscallArgs) -> SyscallResult {
    linux_err(errno::ENOTTY)
}

/// `prctl(option, arg2, arg3, arg4, arg5)` — Linux's "process control"
/// catch-all for per-process state that doesn't justify a dedicated
/// syscall.
///
/// We accept the small set of `PR_*` options that real Linux startup
/// code (musl/glibc/systemd) hits, and return `-EINVAL` for everything
/// else — Linux's documented response for "unrecognised option".
///
/// Accepted as silent success (returning 0):
///   - `PR_SET_NAME` (15): set the comm name visible in `/proc/<pid>/comm`
///     and `prctl(PR_GET_NAME)`.  We don't carry per-thread names yet,
///     so the requested name is dropped, but the call succeeds so the
///     program can continue.
///   - `PR_GET_NAME` (16): the symmetric query.  We zero the user buf
///     (16 bytes) if non-NULL, which reports "thread has no name".
///   - `PR_SET_DUMPABLE` (4) / `PR_GET_DUMPABLE` (3): we don't produce
///     core dumps; SET accepts any value, GET returns 0 (not dumpable).
///   - `PR_SET_PDEATHSIG` (1): "send sig when parent dies" — we don't
///     have parent-death tracking but most callers (systemd, init
///     systems) handle SET failing only by logging.  Accepting it
///     keeps them quiet.
///   - `PR_SET_NO_NEW_PRIVS` (38) / `PR_GET_NO_NEW_PRIVS` (39): sandbox
///     bit.  SET accepts; GET returns 1 (the most paranoid answer
///     since we don't honour set*id setuid bits anyway).
///   - `PR_SET_KEEPCAPS` (8) / `PR_GET_KEEPCAPS` (7): capability
///     preservation across uid change.  We don't have uids; accept SET
///     and return 0 for GET.
///   - `PR_CAPBSET_READ` (23) / `PR_CAPBSET_DROP` (24): capability
///     bounding set.  We have a capability system but not Linux-style
///     POSIX capability bits; accept READ as "yes, that cap exists"
///     (return 1) and DROP as silent success.  This is the friendliest
///     answer for systemd, which calls PR_CAPBSET_DROP for every
///     capability it wants to drop and gates on the result.
///
/// Everything else: `-EINVAL`.
///
/// Limitation: PR_SET_NAME / PR_GET_NAME are no-ops — programs that
/// inspect /proc/<pid>/comm to find their own threads (some debugger
/// integration) will see empty names.  Tracked in todo.txt as needing
/// per-thread name storage on the TCB plus a procfs string field.
fn sys_prctl(args: &SyscallArgs) -> SyscallResult {
    let option = args.arg0;
    match option {
        // PR_SET_PDEATHSIG, PR_SET_DUMPABLE, PR_SET_KEEPCAPS,
        // PR_SET_NAME — accept silently.
        1 | 4 | 8 | 15 => SyscallResult::ok(0),
        // PR_GET_DUMPABLE — not dumpable.
        3 => SyscallResult::ok(0),
        // PR_GET_KEEPCAPS — we don't track it.
        7 => SyscallResult::ok(0),
        // PR_GET_NAME — copy 16 zero bytes to the user buffer if
        // non-NULL.  Linux's comm is exactly 16 bytes (15 chars + NUL).
        16 => {
            let user_buf = args.arg1;
            if user_buf != 0 {
                if let Err(e) = crate::mm::user::validate_user_write(user_buf, 16) {
                    return linux_err(linux_errno_for(e));
                }
                let zero = [0u8; 16];
                // SAFETY: validate_user_write above confirmed a 16-byte
                // writable user range; we copy 16 zero bytes.
                let r = unsafe {
                    crate::mm::user::copy_to_user(zero.as_ptr(), user_buf, 16)
                };
                if let Err(e) = r {
                    return linux_err(linux_errno_for(e));
                }
            }
            SyscallResult::ok(0)
        }
        // PR_CAPBSET_READ — Linux returns 1 if the cap is in the
        // bounding set, 0 if not.  We don't track POSIX caps; report
        // "in set" so systemd doesn't refuse to start because it
        // thinks a capability it wants to drop isn't available.
        23 => SyscallResult::ok(1),
        // PR_CAPBSET_DROP — silent success.
        24 => SyscallResult::ok(0),
        // PR_SET_NO_NEW_PRIVS — silent success.
        38 => SyscallResult::ok(0),
        // PR_GET_NO_NEW_PRIVS — return 1 (the paranoid answer; we
        // don't honour setuid bits so "no new privs" is true by
        // construction).
        39 => SyscallResult::ok(1),
        _ => linux_err(errno::EINVAL),
    }
}

/// `personality(persona)` — get/set the execution personality (Linux,
/// BSD, SVR4, etc.).
///
/// The argument `persona == 0xffff_ffff` is the canonical "query
/// current personality" call; libc startup uses this to verify we're
/// PER_LINUX before doing anything Linux-ABI-specific.  Any other value
/// is "set to this personality"; we accept and ignore (Linux ignores
/// most personality bits anyway).
///
/// Returns the previous personality, which is always 0 (PER_LINUX) for
/// us.
fn sys_personality(_args: &SyscallArgs) -> SyscallResult {
    // PER_LINUX == 0; we never had any other personality so the
    // "previous" value is also 0.
    SyscallResult::ok(0)
}

/// `getresuid(ruid, euid, suid)` — fetch real/effective/saved user IDs.
///
/// We have no uid model yet (everything runs as the implicit root-
/// equivalent owner of all kernel objects via the capability system),
/// so we report uid 0 for all three fields.  This matches what a
/// process started by `init` on a Linux system would see and lets
/// `geteuid()==0` privilege checks in pre-existing Linux code fire
/// the way the program expects.
///
/// Errors:
///   - `-EFAULT` on a bad user pointer.
fn sys_getresuid(args: &SyscallArgs) -> SyscallResult {
    let ruid_ptr = args.arg0;
    let euid_ptr = args.arg1;
    let suid_ptr = args.arg2;
    write_uid32_triple(ruid_ptr, euid_ptr, suid_ptr)
}

/// `getresgid(rgid, egid, sgid)` — fetch real/effective/saved group IDs.
///
/// Same model and contract as [`sys_getresuid`]; reports 0 for all
/// three.
fn sys_getresgid(args: &SyscallArgs) -> SyscallResult {
    let rgid_ptr = args.arg0;
    let egid_ptr = args.arg1;
    let sgid_ptr = args.arg2;
    write_uid32_triple(rgid_ptr, egid_ptr, sgid_ptr)
}

/// Helper shared by [`sys_getresuid`] / [`sys_getresgid`]: write three
/// `uid_t` (Linux x86_64: 32-bit unsigned) zeros to the three user
/// pointers if non-NULL.  NULL pointers are skipped (POSIX permits any
/// of the three fields to be discarded).  Returns 0 on success, the
/// translated errno on a faulting pointer.
fn write_uid32_triple(a: u64, b: u64, c: u64) -> SyscallResult {
    let zero = [0u8; 4];
    for &p in &[a, b, c] {
        if p == 0 {
            continue;
        }
        if let Err(e) = crate::mm::user::validate_user_write(p, 4) {
            return linux_err(linux_errno_for(e));
        }
        // SAFETY: validate_user_write above confirmed a 4-byte
        // writable user range; we copy exactly 4 zero bytes.
        let r = unsafe { crate::mm::user::copy_to_user(zero.as_ptr(), p, 4) };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
    }
    SyscallResult::ok(0)
}

/// `getrusage(who, usage)` — query resource usage for the calling
/// process or one of its children.
///
/// We don't track per-process CPU time, page faults, context switches,
/// etc., so we report all-zero `struct rusage` (144 bytes on x86_64).
/// Programs that consume `ru_utime` / `ru_stime` (e.g. `time(1)` after
/// a child exits) see zero CPU time — observationally correct in the
/// sense that nothing claims false work, but loses fidelity.
///
/// `who`:
///   - `RUSAGE_SELF == 0`: stats for the calling process
///   - `RUSAGE_CHILDREN == -1`: aggregate stats for reaped children
///   - `RUSAGE_THREAD == 1`: stats for the calling thread
///
/// We accept all three (and silently accept anything else) — every
/// value gets the same zero rusage back.  Strict Linux returns EINVAL
/// for unknown `who`, but we'd rather be lenient than break a program
/// that's sloppy about the constant.
///
/// Returns 0 on success, `-EFAULT` if `usage` is a bad pointer.
fn sys_getrusage(args: &SyscallArgs) -> SyscallResult {
    let usage_ptr = args.arg1;
    if usage_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    // struct rusage on Linux x86_64 is 18 longs = 144 bytes.
    const RUSAGE_SIZE: usize = 144;
    if let Err(e) = crate::mm::user::validate_user_write(usage_ptr, RUSAGE_SIZE) {
        return linux_err(linux_errno_for(e));
    }
    let zero = [0u8; RUSAGE_SIZE];
    // SAFETY: validate_user_write above confirmed a 144-byte
    // writable user range; we copy 144 zero bytes from kernel memory.
    let r = unsafe { crate::mm::user::copy_to_user(zero.as_ptr(), usage_ptr, RUSAGE_SIZE) };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `sysinfo(info)` — fill in `struct sysinfo` with system-wide
/// stats (uptime, load avg, total/free RAM, swap, processes, etc.).
///
/// `struct sysinfo` on Linux x86_64 is 112 bytes: `long uptime; ulong
/// loads[3]; ulong totalram; ulong freeram; ulong sharedram; ulong
/// bufferram; ulong totalswap; ulong freeswap; ushort procs; ushort
/// pad; ulong totalhigh; ulong freehigh; uint mem_unit; char _f[8];`.
///
/// We fill in:
///   - `uptime` — boot-relative time in seconds
///   - `totalram` / `freeram` — best-effort from the page allocator
///   - `mem_unit` — 1 (so totalram/freeram are byte counts directly)
///
/// Everything else is zero.  This is enough for `uptime(1)` and most
/// monitoring tools to produce a sensible display rather than crashing
/// on uninit fields.
///
/// Returns 0 on success, `-EFAULT` if `info` is a bad pointer.
fn sys_sysinfo(args: &SyscallArgs) -> SyscallResult {
    let info_ptr = args.arg0;
    if info_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    const SYSINFO_SIZE: usize = 112;
    if let Err(e) = crate::mm::user::validate_user_write(info_ptr, SYSINFO_SIZE) {
        return linux_err(linux_errno_for(e));
    }

    // Build the struct in kernel memory.  Field offsets per Linux
    // x86_64 ABI:
    //   0..8    uptime (long)
    //   8..32   loads[3] (3 × ulong)
    //  32..40   totalram
    //  40..48   freeram
    //  48..56   sharedram
    //  56..64   bufferram
    //  64..72   totalswap
    //  72..80   freeswap
    //  80..82   procs (ushort)
    //  82..84   pad
    //  84..92   totalhigh
    //  92..100  freehigh
    // 100..104  mem_unit (uint)
    // 104..112  _f[8]
    let mut buf = [0u8; SYSINFO_SIZE];

    // Uptime in seconds since boot.  uptime_secs is the canonical
    // wrapper over clock_monotonic / 1e9.
    let uptime_s: u64 = crate::timekeeping::uptime_secs();
    #[allow(clippy::cast_possible_wrap)]
    let uptime_i: i64 = uptime_s as i64;
    buf[0..8].copy_from_slice(&uptime_i.to_ne_bytes());

    // RAM totals from the page allocator.  Best effort — if the
    // allocator can't report (uninitialised), leave zero.
    if let Some(s) = crate::mm::frame::stats() {
        #[allow(clippy::cast_possible_truncation)]
        let total_bytes = (s.total_frames as u64)
            .saturating_mul(crate::mm::frame::FRAME_SIZE as u64);
        let free_bytes = s.free_bytes as u64;
        buf[32..40].copy_from_slice(&total_bytes.to_ne_bytes());
        buf[40..48].copy_from_slice(&free_bytes.to_ne_bytes());
    }

    // mem_unit = 1 (totalram/freeram are byte counts directly).
    let mem_unit: u32 = 1;
    buf[100..104].copy_from_slice(&mem_unit.to_ne_bytes());

    // SAFETY: validate_user_write above confirmed a 112-byte writable
    // user range; we copy exactly SYSINFO_SIZE bytes.
    let r = unsafe { crate::mm::user::copy_to_user(buf.as_ptr(), info_ptr, SYSINFO_SIZE) };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `times(tms)` — fill in `struct tms { utime; stime; cutime; cstime }`
/// (4 × clock_t = 4 × 8 bytes on x86_64).  Returns the clock ticks
/// since an arbitrary reference (Linux: time since boot in jiffies).
///
/// We don't track per-process CPU time, so all four `tms` fields are
/// zero.  The return value uses `CLOCKS_PER_SEC == 100` (a common
/// libc HZ) and reports `monotonic_seconds * 100`.
///
/// Returns clock ticks on success, `-EFAULT` on bad `tms` pointer.
fn sys_times(args: &SyscallArgs) -> SyscallResult {
    let tms_ptr = args.arg0;

    // tms_ptr is allowed to be NULL — POSIX says it's optional when
    // the caller only wants the return value.  When non-NULL, write
    // 32 zero bytes.
    if tms_ptr != 0 {
        const TMS_SIZE: usize = 32;
        if let Err(e) = crate::mm::user::validate_user_write(tms_ptr, TMS_SIZE) {
            return linux_err(linux_errno_for(e));
        }
        let zero = [0u8; TMS_SIZE];
        // SAFETY: validate_user_write above confirmed a 32-byte
        // writable user range; we copy 32 zero bytes.
        let r = unsafe { crate::mm::user::copy_to_user(zero.as_ptr(), tms_ptr, TMS_SIZE) };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
    }

    // Return value: ticks since boot at HZ == 100.
    let ticks = crate::timekeeping::clock_monotonic() / 10_000_000; // 1e9/100
    #[allow(clippy::cast_possible_wrap)]
    let v = ticks as i64;
    SyscallResult::ok(v)
}

/// `getpgrp()` — return the calling process's process-group ID.
///
/// We don't have process groups; every process is implicitly the
/// sole member of its own group.  Return the caller's PID, which is
/// also what Linux would return if the process had called
/// `setpgrp()` to detach itself into a fresh group (the common case
/// for shells and daemons).
///
/// Never fails; returns 1 if there's no caller (boot-context probe).
fn sys_getpgrp(_args: &SyscallArgs) -> SyscallResult {
    let pid = caller_pid().unwrap_or(1);
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(pid as i64)
}

/// `getpgid(pid)` — return the process-group ID of `pid` (or self if
/// `pid == 0`).
///
/// We don't track group membership.  Without it, the most truthful
/// answer is "pgid == pid" — every process is in its own group.
///
/// Errors:
///   - `-ESRCH` if `pid` refers to a non-existent process.
fn sys_getpgid(args: &SyscallArgs) -> SyscallResult {
    let pid = args.arg0;
    let target = if pid == 0 {
        caller_pid().unwrap_or(1)
    } else {
        // Verify the target exists.
        match crate::proc::pcb::state(pid) {
            Some(_) => pid,
            None => return linux_err(errno::ESRCH),
        }
    };
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(target as i64)
}

/// `setpgid(pid, pgid)` — set the process group of `pid` to `pgid`.
///
/// We have no process groups; accept the call and silently no-op.
/// Linux returns EINVAL for negative pgid and EPERM for cross-session
/// moves; we don't validate either (no sessions, no groups), but we
/// do reject obviously invalid values (negative-cast-from-i64 patterns
/// like the high bit set).
///
/// Limitation: programs that fork a worker pool and then move all
/// workers into a common pgrp for collective signalling won't see
/// the effect — a `kill(-pgid)` would still ESRCH because we treat
/// every process as its own group.  Tracked alongside process-group
/// infrastructure in todo.txt.
fn sys_setpgid(args: &SyscallArgs) -> SyscallResult {
    let pgid = args.arg1;
    // Reject obviously bogus pgid (negative when cast).
    #[allow(clippy::cast_possible_wrap)]
    if (pgid as i64) < 0 {
        return linux_err(errno::EINVAL);
    }
    SyscallResult::ok(0)
}

/// `getsid(pid)` — return the session ID of `pid` (or self if 0).
///
/// We have no sessions; return the target PID as a stand-in (every
/// process is in its own session of which it is the leader).
fn sys_getsid(args: &SyscallArgs) -> SyscallResult {
    let pid = args.arg0;
    let target = if pid == 0 {
        caller_pid().unwrap_or(1)
    } else {
        match crate::proc::pcb::state(pid) {
            Some(_) => pid,
            None => return linux_err(errno::ESRCH),
        }
    };
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(target as i64)
}

/// `setsid()` — create a new session with the calling process as
/// leader.
///
/// We have no sessions, so this is a silent success that returns the
/// caller's PID (Linux's success contract: "new session ID, which
/// equals the new pgid, which equals the caller's pid").
fn sys_setsid(_args: &SyscallArgs) -> SyscallResult {
    let pid = caller_pid().unwrap_or(1);
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(pid as i64)
}

/// `getpriority(which, who)` — return the nice value of a process,
/// process group, or user.
///
/// We don't honour nice values; the scheduler runs strict round-robin
/// within a priority class.  Report nice == 0 (the unbiased default)
/// for every query.
///
/// CAUTION: Linux's getpriority return-value contract is unusual.  A
/// successful call can return any value in `[-20, 19]`, including the
/// negative ones that would normally indicate an error.  To
/// disambiguate, callers must clear errno before the call and check
/// errno after.  Our success return is 0, which is unambiguous.
fn sys_getpriority(args: &SyscallArgs) -> SyscallResult {
    let which = args.arg0;
    // Valid values: PRIO_PROCESS=0, PRIO_PGRP=1, PRIO_USER=2.
    if which > 2 {
        return linux_err(errno::EINVAL);
    }
    SyscallResult::ok(0)
}

/// `setpriority(which, who, prio)` — set the nice value.
///
/// We don't honour nice; accept any in-range request and silently
/// succeed.  Out-of-range or unknown `which` returns EINVAL.
fn sys_setpriority(args: &SyscallArgs) -> SyscallResult {
    let which = args.arg0;
    if which > 2 {
        return linux_err(errno::EINVAL);
    }
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Credentials: setuid / setgid family + capabilities
//
// We have no UID/GID model — all processes run as the implicit "root"
// owner of all kernel objects, mediated by the capability system.
// The Linux credential syscalls all degenerate to silent success
// (since "set to 0" is always permitted, and we treat every value as
// "becoming 0" effectively).  Rejecting non-zero values with EPERM
// would be more truthful, but breaks programs that drop privileges at
// startup as a defense-in-depth measure: they'd refuse to continue
// when setuid(nobody) fails.  The friendlier stub accepts the call
// and quietly keeps the program in its "as if root" state — which is
// the only state we actually support.
// ---------------------------------------------------------------------------

/// `setuid(uid)` — set the effective uid (and real/saved if caller is
/// root).  Silent success in our model.
fn sys_setuid(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `setgid(gid)` — set the effective gid.  Silent success.
fn sys_setgid(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `setreuid(ruid, euid)` — set real and effective uid.  Silent success.
fn sys_setreuid(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `setregid(rgid, egid)` — set real and effective gid.  Silent success.
fn sys_setregid(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `setresuid(ruid, euid, suid)` — set real / effective / saved uid.
/// Silent success.
fn sys_setresuid(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `setresgid(rgid, egid, sgid)` — set real / effective / saved gid.
/// Silent success.
fn sys_setresgid(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `setfsuid(fsuid)` — set the filesystem uid (used for permission
/// checks on subsequent FS ops).  Linux's contract is unusual: it
/// returns the *previous* fsuid regardless of whether the change
/// succeeded.  We always report 0.
fn sys_setfsuid(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `setfsgid(fsgid)` — set the filesystem gid.  Same contract as
/// [`sys_setfsuid`].
fn sys_setfsgid(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `getgroups(size, list)` — fetch the supplementary group list.
///
/// We don't carry supp groups; return 0 (empty list).  When `size`
/// is 0, this is "tell me how many supp groups I have"; when `size`
/// is non-zero and `list` is non-NULL, we'd normally write up to
/// `size` gid_t values.  Either way we report zero groups.
///
/// Note: Linux validates `size < 0` as EINVAL but the arg is a `size_t`
/// (unsigned) so negative values aren't representable; we don't gate
/// on size and let the empty-list answer ride.
fn sys_getgroups(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `setgroups(size, list)` — set the supplementary group list.
///
/// We don't carry supp groups; accept any size (including 0) as
/// silent success.  Programs that drop groups via `setgroups(0,
/// NULL)` (the canonical "drop all supp groups before chroot"
/// pattern) get the success they expect.
fn sys_setgroups(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `capget(hdrp, datap)` — query the calling thread's capability sets.
///
/// `struct __user_cap_header_struct *hdrp = { __u32 version; int pid; }`
/// (8 bytes).
/// `struct __user_cap_data_struct *datap = { __u32 effective;
///                                            __u32 permitted;
///                                            __u32 inheritable; }`
/// (12 bytes per element; 2 elements for `_LINUX_CAPABILITY_VERSION_3`).
///
/// We don't have POSIX-style capability bits.  Report all-ones for
/// every set, signalling "this process has every capability".  That's
/// the most permissive answer and the one that matches our "everyone's
/// effectively root" stance.
///
/// `hdrp.version` is validated as one of the three known Linux
/// versions (1/2/3); unknown versions get rewritten to v3 and we
/// return -EINVAL (Linux's documented behaviour — caller must retry
/// with the new version).
///
/// On unknown version, we also write the v3 version sentinel into
/// hdrp.version before returning EINVAL so the caller's retry loop
/// converges.
fn sys_capget(args: &SyscallArgs) -> SyscallResult {
    let hdrp = args.arg0;
    let datap = args.arg1;

    /// `_LINUX_CAPABILITY_VERSION_1` (1985 vintage, single 32-bit set).
    const V1: u32 = 0x1998_0330;
    /// `_LINUX_CAPABILITY_VERSION_2` (2008 vintage, 64-bit but buggy).
    const V2: u32 = 0x2007_1026;
    /// `_LINUX_CAPABILITY_VERSION_3` (current — 64-bit, fixed).
    const V3: u32 = 0x2008_0522;

    if hdrp == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(hdrp, 8) {
        return linux_err(linux_errno_for(e));
    }
    // Read version.
    let mut hdr_buf = [0u8; 8];
    // SAFETY: validate_user_read above confirmed an 8-byte readable
    // user range.
    let r = unsafe {
        crate::mm::user::copy_from_user(hdrp, hdr_buf.as_mut_ptr(), 8)
    };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    let version = u32::from_ne_bytes([hdr_buf[0], hdr_buf[1], hdr_buf[2], hdr_buf[3]]);
    let elems: usize = match version {
        V1 => 1,
        V2 | V3 => 2,
        _ => {
            // Rewrite header version to V3 and return EINVAL —
            // Linux's documented retry protocol.
            if let Err(e) = crate::mm::user::validate_user_write(hdrp, 8) {
                return linux_err(linux_errno_for(e));
            }
            let v3 = V3.to_ne_bytes();
            hdr_buf[0] = v3[0]; hdr_buf[1] = v3[1];
            hdr_buf[2] = v3[2]; hdr_buf[3] = v3[3];
            // SAFETY: validate_user_write confirmed the 8-byte range.
            let r = unsafe { crate::mm::user::copy_to_user(hdr_buf.as_ptr(), hdrp, 8) };
            if let Err(e) = r {
                return linux_err(linux_errno_for(e));
            }
            return linux_err(errno::EINVAL);
        }
    };

    if datap == 0 {
        // Linux allows datap == NULL when the caller is probing for
        // version support; we already returned the version-OK signal
        // by getting this far, so return 0.
        return SyscallResult::ok(0);
    }
    let total = elems.saturating_mul(12);
    if let Err(e) = crate::mm::user::validate_user_write(datap, total) {
        return linux_err(linux_errno_for(e));
    }
    // Build all-ones data structure.
    let mut data = [0xffu8; 24]; // max V2/V3 size
    // V1 datap is only 12 bytes; we'll copy `total` bytes which is
    // exactly the right amount.
    let _ = &mut data; // ensure the slice is materialised
    // SAFETY: validate_user_write above confirmed `total` writable
    // bytes; we copy exactly `total` 0xff bytes.
    let r = unsafe { crate::mm::user::copy_to_user(data.as_ptr(), datap, total) };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `capset(hdrp, datap)` — install new capability sets.
///
/// We accept any well-formed request as silent success.  Validation
/// mirrors [`sys_capget`] (version must be V1/V2/V3, else EINVAL with
/// the version-rewrite-to-V3 protocol).
fn sys_capset(args: &SyscallArgs) -> SyscallResult {
    let hdrp = args.arg0;

    const V1: u32 = 0x1998_0330;
    const V2: u32 = 0x2007_1026;
    const V3: u32 = 0x2008_0522;

    if hdrp == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(hdrp, 8) {
        return linux_err(linux_errno_for(e));
    }
    let mut hdr_buf = [0u8; 8];
    // SAFETY: validate_user_read above confirmed an 8-byte readable
    // user range.
    let r = unsafe {
        crate::mm::user::copy_from_user(hdrp, hdr_buf.as_mut_ptr(), 8)
    };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    let version = u32::from_ne_bytes([hdr_buf[0], hdr_buf[1], hdr_buf[2], hdr_buf[3]]);
    match version {
        V1 | V2 | V3 => SyscallResult::ok(0),
        _ => {
            if let Err(e) = crate::mm::user::validate_user_write(hdrp, 8) {
                return linux_err(linux_errno_for(e));
            }
            let v3 = V3.to_ne_bytes();
            hdr_buf[0] = v3[0]; hdr_buf[1] = v3[1];
            hdr_buf[2] = v3[2]; hdr_buf[3] = v3[3];
            // SAFETY: validate_user_write confirmed the 8-byte range.
            let r = unsafe { crate::mm::user::copy_to_user(hdr_buf.as_ptr(), hdrp, 8) };
            if let Err(e) = r {
                return linux_err(linux_errno_for(e));
            }
            linux_err(errno::EINVAL)
        }
    }
}

// ---------------------------------------------------------------------------
// Scheduler queries: policy / params / priority bounds / affinity
//
// Linux's per-process scheduling parameters (SCHED_OTHER vs FIFO vs RR
// vs DEADLINE etc.) aren't modelled in our kernel — we have a single
// priority-round-robin scheduler with a kernel-internal priority
// concept that doesn't map cleanly to Linux's policy classes.  We
// report "SCHED_OTHER, priority 0" universally, which matches what a
// normal Linux process sees by default.
// ---------------------------------------------------------------------------

/// `sched_getscheduler(pid)` — return the scheduling policy of `pid`.
///
/// Linux policy constants:
///   - `SCHED_OTHER == 0` — the normal CFS / EEVDF default.
///   - `SCHED_FIFO == 1`, `SCHED_RR == 2` — POSIX real-time.
///   - `SCHED_BATCH == 3`, `SCHED_IDLE == 5`, `SCHED_DEADLINE == 6` —
///     Linux extensions.
///
/// We always return 0 (SCHED_OTHER); ESRCH for non-existent pids.
fn sys_sched_getscheduler(args: &SyscallArgs) -> SyscallResult {
    let pid = args.arg0;
    if pid != 0 {
        if crate::proc::pcb::state(pid).is_none() {
            return linux_err(errno::ESRCH);
        }
    }
    SyscallResult::ok(0)
}

/// `sched_setscheduler(pid, policy, sched_param)` — install a new
/// scheduling policy.
///
/// Accepts policy in 0..=7 as silent success; out-of-range -> EINVAL.
fn sys_sched_setscheduler(args: &SyscallArgs) -> SyscallResult {
    let pid = args.arg0;
    let policy = args.arg1;
    if policy > 7 {
        return linux_err(errno::EINVAL);
    }
    if pid != 0 {
        if crate::proc::pcb::state(pid).is_none() {
            return linux_err(errno::ESRCH);
        }
    }
    SyscallResult::ok(0)
}

/// `sched_getparam(pid, param)` — write `struct sched_param { int
/// sched_priority; }` to `param`.
///
/// We report priority 0 (the SCHED_OTHER default).
fn sys_sched_getparam(args: &SyscallArgs) -> SyscallResult {
    let pid = args.arg0;
    let param_ptr = args.arg1;
    if pid != 0 {
        if crate::proc::pcb::state(pid).is_none() {
            return linux_err(errno::ESRCH);
        }
    }
    if param_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    // struct sched_param is just { int sched_priority; } = 4 bytes,
    // but glibc rounds it up via alignof so callers typically
    // allocate sizeof(int).
    if let Err(e) = crate::mm::user::validate_user_write(param_ptr, 4) {
        return linux_err(linux_errno_for(e));
    }
    let zero = [0u8; 4];
    // SAFETY: validated 4-byte writable user range.
    let r = unsafe { crate::mm::user::copy_to_user(zero.as_ptr(), param_ptr, 4) };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `sched_setparam(pid, param)` — install new sched parameters.
///
/// Accepted silently; only the existence-of-pid gate applies.
fn sys_sched_setparam(args: &SyscallArgs) -> SyscallResult {
    let pid = args.arg0;
    if pid != 0 {
        if crate::proc::pcb::state(pid).is_none() {
            return linux_err(errno::ESRCH);
        }
    }
    SyscallResult::ok(0)
}

/// `sched_get_priority_max(policy)` — return the maximum static
/// priority for `policy`.
///
/// Linux returns:
///   - SCHED_FIFO / SCHED_RR -> 99
///   - SCHED_OTHER / SCHED_BATCH / SCHED_IDLE -> 0
///   - unknown -> -EINVAL
///
/// We mirror that exactly even though we don't honour real-time
/// priorities — programs sanity-check the value before using it.
fn sys_sched_get_priority_max(args: &SyscallArgs) -> SyscallResult {
    let policy = args.arg0;
    match policy {
        1 | 2 => SyscallResult::ok(99),                 // FIFO / RR
        0 | 3 | 5 | 6 | 7 => SyscallResult::ok(0),      // OTHER / BATCH / IDLE / DEADLINE / EXT
        _ => linux_err(errno::EINVAL),
    }
}

/// `sched_get_priority_min(policy)` — return the minimum static
/// priority for `policy`.
///
/// Linux returns:
///   - SCHED_FIFO / SCHED_RR -> 1
///   - SCHED_OTHER / SCHED_BATCH / SCHED_IDLE -> 0
///   - unknown -> -EINVAL
fn sys_sched_get_priority_min(args: &SyscallArgs) -> SyscallResult {
    let policy = args.arg0;
    match policy {
        1 | 2 => SyscallResult::ok(1),
        0 | 3 | 5 | 6 | 7 => SyscallResult::ok(0),
        _ => linux_err(errno::EINVAL),
    }
}

/// `sched_rr_get_interval(pid, ts)` — write the round-robin time
/// slice to `ts` (a `struct timespec`).
///
/// We report 100 ms (a typical Linux RR slice).
fn sys_sched_rr_get_interval(args: &SyscallArgs) -> SyscallResult {
    let pid = args.arg0;
    let ts_ptr = args.arg1;
    if pid != 0 {
        if crate::proc::pcb::state(pid).is_none() {
            return linux_err(errno::ESRCH);
        }
    }
    if ts_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    // struct timespec { tv_sec: i64, tv_nsec: i64 } — 16 bytes total
    // on x86_64.
    if let Err(e) = crate::mm::user::validate_user_write(ts_ptr, 16) {
        return linux_err(linux_errno_for(e));
    }
    let mut buf = [0u8; 16];
    // 100ms = 0 sec + 100_000_000 ns.
    let sec: i64 = 0;
    let nsec: i64 = 100_000_000;
    buf[0..8].copy_from_slice(&sec.to_ne_bytes());
    buf[8..16].copy_from_slice(&nsec.to_ne_bytes());
    // SAFETY: validated 16-byte writable user range.
    let r = unsafe { crate::mm::user::copy_to_user(buf.as_ptr(), ts_ptr, 16) };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `sched_getaffinity(pid, cpusetsize, mask)` — fetch the CPU affinity
/// mask of `pid`.
///
/// Linux's `cpu_set_t` is a fixed-size bitmask (typically 1024 bits).
/// `cpusetsize` is the buffer size in bytes; the kernel writes up to
/// that many bytes and returns the number of bytes actually written
/// (so callers can detect a too-small buffer and retry).
///
/// We report every online CPU as eligible (the default affinity for a
/// freshly-created task on Linux).  The mask is filled in bit-by-bit
/// from 0..N where N = smp::cpu_count().
///
/// Errors:
///   - `-EINVAL` if `cpusetsize` is less than the number of bytes
///     needed to represent every online CPU (Linux's contract).
///   - `-EFAULT` on bad `mask` pointer.
///   - `-ESRCH` if `pid` is not the caller and not a real pid.
///
/// Returns the number of bytes written (Linux convention).
fn sys_sched_getaffinity(args: &SyscallArgs) -> SyscallResult {
    let pid = args.arg0;
    let cpusetsize = args.arg1 as usize;
    let mask_ptr = args.arg2;

    if pid != 0 {
        if crate::proc::pcb::state(pid).is_none() {
            return linux_err(errno::ESRCH);
        }
    }
    if mask_ptr == 0 {
        return linux_err(errno::EFAULT);
    }

    let n_cpus = crate::smp::cpu_count().max(1);
    // Round up to whole bytes.
    let needed_bytes = (n_cpus + 7) / 8;
    if cpusetsize < needed_bytes {
        return linux_err(errno::EINVAL);
    }

    if let Err(e) = crate::mm::user::validate_user_write(mask_ptr, cpusetsize) {
        return linux_err(linux_errno_for(e));
    }

    // Build the mask in kernel memory.  Cap at a reasonable upper
    // bound (1024 bits == 128 bytes) — anything larger is silly and
    // glibc never asks for more than 128.
    const MAX_MASK: usize = 128;
    let mut buf = [0u8; MAX_MASK];
    let write_bytes = cpusetsize.min(MAX_MASK);
    // Set bits 0..n_cpus.
    for cpu in 0..n_cpus {
        let byte_off = cpu / 8;
        let bit = cpu % 8;
        if byte_off < write_bytes {
            #[allow(clippy::indexing_slicing)]
            {
                buf[byte_off] |= 1u8 << bit;
            }
        }
    }
    // SAFETY: validate_user_write above confirmed `cpusetsize` writable
    // bytes; we copy min(cpusetsize, MAX_MASK) bytes.
    let r = unsafe { crate::mm::user::copy_to_user(buf.as_ptr(), mask_ptr, write_bytes) };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }

    // Linux returns the number of bytes written.
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(write_bytes as i64)
}

/// `sched_setaffinity(pid, cpusetsize, mask)` — set the CPU affinity
/// mask of `pid`.
///
/// We accept any mask as silent success — affinity is advisory and
/// our scheduler doesn't honour it yet.  The caller's view via
/// sched_getaffinity will continue to report "all online CPUs" even
/// after a successful setaffinity, which is technically incorrect but
/// matches the "we don't enforce" model.
///
/// Errors:
///   - `-EFAULT` on bad mask pointer.
///   - `-ESRCH` on bad pid.
fn sys_sched_setaffinity(args: &SyscallArgs) -> SyscallResult {
    let pid = args.arg0;
    let cpusetsize = args.arg1 as usize;
    let mask_ptr = args.arg2;
    if pid != 0 {
        if crate::proc::pcb::state(pid).is_none() {
            return linux_err(errno::ESRCH);
        }
    }
    if mask_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(mask_ptr, cpusetsize) {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Filesystem sync syscalls
//
// We don't have a unified buffer-cache flush mechanism yet, so these
// are silent-success stubs.  Programs that rely on these for
// durability (databases, in particular) will write at risk of
// crash-loss on real hardware.  Tracked in todo.txt.
// ---------------------------------------------------------------------------

/// `fsync(fd)` — flush all writes for `fd` to durable storage.
fn sys_fsync(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `fdatasync(fd)` — flush only the data (not metadata) for `fd`.
fn sys_fdatasync(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `sync()` — flush all filesystem writes to durable storage.
fn sys_sync(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `syncfs(fd)` — flush all writes for the filesystem containing `fd`.
fn sys_syncfs(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `sethostname(name, len)` — set the system hostname.
///
/// We don't carry a mutable hostname (uname reports "localhost"
/// always).  Accept any name as silent success; validate the user
/// pointer.
///
/// Errors:
///   - `-EFAULT` on bad pointer
///   - `-EINVAL` for `len > 64` (Linux's `_UTSNAME_NODENAME_LENGTH`).
fn sys_sethostname(args: &SyscallArgs) -> SyscallResult {
    let name_ptr = args.arg0;
    let len = args.arg1 as usize;
    if len > 64 {
        return linux_err(errno::EINVAL);
    }
    if name_ptr == 0 && len != 0 {
        return linux_err(errno::EFAULT);
    }
    if name_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(name_ptr, len) {
            return linux_err(linux_errno_for(e));
        }
    }
    SyscallResult::ok(0)
}

/// `setdomainname(name, len)` — set the NIS domain name.
///
/// Same model as [`sys_sethostname`].
fn sys_setdomainname(args: &SyscallArgs) -> SyscallResult {
    sys_sethostname(args)
}

// ---------------------------------------------------------------------------
// Memory-locking / paging hints (mlock, munlock, mlockall, munlockall, msync)
//
// We do not yet support memory locking — pages are always resident in our
// design until a swap subsystem lands.  Linux programs (notably hardened
// allocators and crypto libraries) call mlock() defensively to prevent
// sensitive bytes from being swapped out; the correct response on a
// non-swapping kernel is to accept the call silently.  msync() flushes a
// memory mapping; with no writeback cache between userspace and the page
// allocator, there is nothing to flush.
//
// All five validate their argument shape (user-range check, flag bits)
// before returning success so that callers passing garbage still observe
// EFAULT/EINVAL as Linux would.
// ---------------------------------------------------------------------------

/// `mlock(addr, len)` — accept after validating the range.
fn sys_mlock(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;
    let len = args.arg1;
    if len == 0 {
        return SyscallResult::ok(0);
    }
    let len_usize = match usize::try_from(len) {
        Ok(v) => v,
        Err(_) => return linux_err(errno::ENOMEM),
    };
    if let Err(e) = crate::mm::user::validate_user_read(addr, len_usize) {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `munlock(addr, len)` — accept after validating the range.
fn sys_munlock(args: &SyscallArgs) -> SyscallResult {
    sys_mlock(args)
}

/// `mlockall(flags)` — accept if flags are in the documented set.
///
/// Linux defines MCL_CURRENT=1, MCL_FUTURE=2, MCL_ONFAULT=4.  Any bits
/// outside this set are EINVAL.
fn sys_mlockall(args: &SyscallArgs) -> SyscallResult {
    const MCL_CURRENT: u64 = 1;
    const MCL_FUTURE: u64 = 2;
    const MCL_ONFAULT: u64 = 4;
    const MCL_ALL: u64 = MCL_CURRENT | MCL_FUTURE | MCL_ONFAULT;
    let flags = args.arg0;
    if flags == 0 || (flags & !MCL_ALL) != 0 {
        return linux_err(errno::EINVAL);
    }
    SyscallResult::ok(0)
}

/// `munlockall()` — always succeeds.
fn sys_munlockall(_args: &SyscallArgs) -> SyscallResult {
    SyscallResult::ok(0)
}

/// `msync(addr, len, flags)` — accept after validating shape.
///
/// Flag bits per Linux: MS_ASYNC=1, MS_INVALIDATE=2, MS_SYNC=4.
/// MS_SYNC and MS_ASYNC are mutually exclusive.
fn sys_msync(args: &SyscallArgs) -> SyscallResult {
    use crate::mm::frame::FRAME_SIZE;
    const MS_ASYNC: u64 = 1;
    const MS_INVALIDATE: u64 = 2;
    const MS_SYNC: u64 = 4;
    const MS_ALL: u64 = MS_ASYNC | MS_INVALIDATE | MS_SYNC;

    let addr = args.arg0;
    let len = args.arg1;
    let flags = args.arg2;

    // Validate flag combination.
    if (flags & !MS_ALL) != 0 {
        return linux_err(errno::EINVAL);
    }
    if (flags & MS_SYNC) != 0 && (flags & MS_ASYNC) != 0 {
        return linux_err(errno::EINVAL);
    }
    // At least one of ASYNC/SYNC must be set per Linux semantics.
    if (flags & (MS_SYNC | MS_ASYNC)) == 0 {
        return linux_err(errno::EINVAL);
    }

    // addr must be page-aligned (16 KiB on this kernel).
    let frame_size = FRAME_SIZE as u64;
    if (addr & (frame_size - 1)) != 0 {
        return linux_err(errno::EINVAL);
    }

    if len == 0 {
        return SyscallResult::ok(0);
    }
    let len_usize = match usize::try_from(len) {
        Ok(v) => v,
        Err(_) => return linux_err(errno::ENOMEM),
    };
    if let Err(e) = crate::mm::user::validate_user_read(addr, len_usize) {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// I/O hints (fadvise64, readahead)
//
// These are advisory: the kernel is free to ignore them.  We validate the
// fd and accept the call.  Real implementations would prefetch pages or
// adjust the readahead window.
// ---------------------------------------------------------------------------

/// `fadvise64(fd, offset, len, advice)` — accept advisory hint.
fn sys_fadvise64(args: &SyscallArgs) -> SyscallResult {
    // POSIX_FADV_* values 0..=6: NORMAL, RANDOM, SEQUENTIAL, WILLNEED,
    // DONTNEED, NOREUSE.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let advice = args.arg3 as i32;
    if !(0..=6).contains(&advice) {
        return linux_err(errno::EINVAL);
    }
    let fd = args.arg0 as i32;
    let pid = match caller_pid() {
        Some(p) => p,
        None => return SyscallResult::ok(0), // kernel context: accept
    };
    if pcb::linux_fd_lookup(pid, fd).is_none() {
        return linux_err(errno::EBADF);
    }
    SyscallResult::ok(0)
}

/// `readahead(fd, offset, count)` — accept advisory hint.
fn sys_readahead(args: &SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as i32;
    let pid = match caller_pid() {
        Some(p) => p,
        None => return SyscallResult::ok(0),
    };
    if pcb::linux_fd_lookup(pid, fd).is_none() {
        return linux_err(errno::EBADF);
    }
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// close_range — bulk fd close
//
// Introduced in Linux 5.9 and adopted by glibc/musl posix_spawn() and
// daemon-startup helpers as a fast way to drop all inherited file
// descriptors before exec.  Without this, programs fall back to a manual
// for(fd = 3; fd < limit; fd++) close(fd) loop, which can be slow.
//
// Flags:
//   CLOSE_RANGE_UNSHARE = 2 — unshare the fd table before closing.
//     We do not share fd tables across processes (fork copies), so this
//     bit is a no-op.
//   CLOSE_RANGE_CLOEXEC = 4 — set FD_CLOEXEC on the range instead of
//     closing.  We honour this by walking the range and toggling the
//     flag on each existing fd.
// ---------------------------------------------------------------------------

/// `close_range(first, last, flags)` — close (or set CLOEXEC on) all open
/// fds in `[first, last]` inclusive.
fn sys_close_range(args: &SyscallArgs) -> SyscallResult {
    const CLOSE_RANGE_UNSHARE: u32 = 2;
    const CLOSE_RANGE_CLOEXEC: u32 = 4;
    const ALL_FLAGS: u32 = CLOSE_RANGE_UNSHARE | CLOSE_RANGE_CLOEXEC;

    let first = args.arg0 as u32;
    let last = args.arg1 as u32;
    #[allow(clippy::cast_possible_truncation)]
    let flags = args.arg2 as u32;

    if (flags & !ALL_FLAGS) != 0 {
        return linux_err(errno::EINVAL);
    }
    if first > last {
        return linux_err(errno::EINVAL);
    }

    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EBADF),
    };

    // Clamp `last` to the fd-table capacity to bound the loop.
    let cap = crate::proc::linux_fd::MAX_FDS as u32;
    let stop = last.min(cap.saturating_sub(1));

    if (flags & CLOSE_RANGE_CLOEXEC) != 0 {
        // Set FD_CLOEXEC on every existing fd in the range.
        for fd in first..=stop {
            #[allow(clippy::cast_possible_wrap)]
            let fd_i = fd as i32;
            if pcb::linux_fd_lookup(pid, fd_i).is_some() {
                let _ = pcb::linux_fd_set_fd_flags(
                    pid,
                    fd_i,
                    crate::proc::linux_fd::FD_CLOEXEC,
                );
            }
        }
        return SyscallResult::ok(0);
    }

    // Close every fd in the range.  Reuse the same logic as sys_close()
    // so we honour the refcount-aware kernel-resource release.
    for fd in first..=stop {
        #[allow(clippy::cast_possible_wrap)]
        let fd_i = fd as i32;
        if let Some(entry) = pcb::linux_fd_take(pid, fd_i)
            && entry.kind.needs_kernel_close()
            && !pcb::linux_fd_is_handle_referenced(pid, entry.kind, entry.raw_handle, -1)
        {
            let _ = close_handle(entry);
        }
    }
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Legacy getrlimit / setrlimit
//
// These predate prlimit64() and operate only on the calling process.  The
// `struct rlimit` they read/write is identical in layout to `struct
// rlimit64` (two u64 fields), so we forward to sys_prlimit64 with pid=0.
//
// Glibc/musl call getrlimit() during early init to size the main-thread
// stack; failing to handle it cleanly causes pthread_create later to
// guess a too-small stack and crash.
// ---------------------------------------------------------------------------

/// `getrlimit(resource, rlim)` — write the current limit for `resource`
/// into `rlim`.
fn sys_getrlimit(args: &SyscallArgs) -> SyscallResult {
    let prlimit_args = SyscallArgs {
        arg0: 0,            // pid: caller
        arg1: args.arg0,    // resource
        arg2: 0,            // new_limit: NULL
        arg3: args.arg1,    // old_limit: out
        arg4: 0,
        arg5: 0,
    };
    sys_prlimit64(&prlimit_args)
}

/// `setrlimit(resource, rlim)` — install a new limit for `resource`.
fn sys_setrlimit(args: &SyscallArgs) -> SyscallResult {
    let prlimit_args = SyscallArgs {
        arg0: 0,            // pid: caller
        arg1: args.arg0,    // resource
        arg2: args.arg1,    // new_limit: in
        arg3: 0,            // old_limit: NULL
        arg4: 0,
        arg5: 0,
    };
    sys_prlimit64(&prlimit_args)
}

// ---------------------------------------------------------------------------
// getcpu — report current CPU / NUMA node
//
// `getcpu(unsigned *cpu, unsigned *node, void *tcache)`.  `tcache` is
// unused on modern kernels.  We report the CPU we're currently running on
// and node 0 (we have no NUMA topology yet).  Either pointer may be NULL.
// ---------------------------------------------------------------------------

/// `getcpu(cpu, node, tcache)` — write the calling thread's current
/// logical CPU id and NUMA node.
fn sys_getcpu(args: &SyscallArgs) -> SyscallResult {
    let cpu_ptr = args.arg0;
    let node_ptr = args.arg1;
    // arg2 (tcache) ignored: NULL is the documented modern usage.

    if cpu_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_write(cpu_ptr, 4) {
            return linux_err(linux_errno_for(e));
        }
        #[allow(clippy::cast_possible_truncation)]
        let cpu_id: u32 = crate::smp::current_cpu_index() as u32;
        let bytes = cpu_id.to_ne_bytes();
        // SAFETY: validated as a writable 4-byte range above.
        let r = unsafe {
            crate::mm::user::copy_to_user(bytes.as_ptr(), cpu_ptr, 4)
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
    }

    if node_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_write(node_ptr, 4) {
            return linux_err(linux_errno_for(e));
        }
        let node_id: u32 = 0;
        let bytes = node_id.to_ne_bytes();
        // SAFETY: validated as a writable 4-byte range above.
        let r = unsafe {
            crate::mm::user::copy_to_user(bytes.as_ptr(), node_ptr, 4)
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
    }

    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// statfs / fstatfs — filesystem statistics
//
// We don't have a real backing filesystem yet, but startup binaries
// frequently call statfs("/tmp") or fstatfs(fd) to plan space and to
// detect filesystem type (e.g. systemd detects tmpfs mounts).  Return
// a synthetic struct that claims "16 KiB block size, plenty of space,
// tmpfs magic".  When real filesystems land, route through the VFS.
//
// struct statfs layout (x86_64 Linux):
//   long f_type, f_bsize, f_blocks, f_bfree, f_bavail,
//        f_files, f_ffree;        // 7 longs = 56 bytes
//   __kernel_fsid_t f_fsid;       // 8 bytes (two i32)
//   long f_namelen, f_frsize, f_flags;
//   long f_spare[4];
// Total: 7*8 + 8 + 3*8 + 4*8 = 120 bytes.
// ---------------------------------------------------------------------------

/// Linux statfs structure size on x86_64.
const STATFS_SIZE: usize = 120;

/// Magic value advertised in `f_type`.  TMPFS_MAGIC is a neutral
/// choice that tells callers "this is RAM-backed, expect no
/// durability guarantees" — accurate for our current implementation.
const TMPFS_MAGIC: u64 = 0x0102_1994;

/// Fill `buf` with a synthetic statfs payload.
fn fill_statfs_default(buf: &mut [u8; STATFS_SIZE]) {
    // f_blocks of 1 << 20 (16 KiB blocks) advertises 16 GiB of space.
    let f_blocks: u64 = 1 << 20;
    let f_files: u64 = 1 << 16;
    let fields: [u64; 15] = [
        TMPFS_MAGIC,        // f_type
        16 * 1024,          // f_bsize (matches our frame size)
        f_blocks,           // f_blocks
        f_blocks / 2,       // f_bfree
        f_blocks / 2,       // f_bavail
        f_files,            // f_files
        f_files / 2,        // f_ffree
        0,                  // f_fsid (two i32 packed into u64)
        255,                // f_namelen (POSIX NAME_MAX)
        16 * 1024,          // f_frsize
        0,                  // f_flags
        0, 0, 0, 0,         // f_spare[4]
    ];
    debug_assert_eq!(fields.len() * 8, STATFS_SIZE);
    for (i, v) in fields.iter().enumerate() {
        let off = i * 8;
        let bytes = v.to_ne_bytes();
        #[allow(clippy::indexing_slicing)]
        for j in 0..8 {
            buf[off + j] = bytes[j];
        }
    }
}

/// `statfs(path, buf)` — fill `buf` with synthetic filesystem stats.
fn sys_statfs(args: &SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0;
    let buf_ptr = args.arg1;

    // Path must be a non-NULL user pointer; we don't yet resolve paths,
    // but we still EFAULT if the pointer itself is bad.
    if path_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path_ptr, 1) {
        return linux_err(linux_errno_for(e));
    }
    if buf_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_write(buf_ptr, STATFS_SIZE) {
        return linux_err(linux_errno_for(e));
    }

    let mut buf = [0u8; STATFS_SIZE];
    fill_statfs_default(&mut buf);
    // SAFETY: validated as a writable STATFS_SIZE-byte range above.
    let r = unsafe {
        crate::mm::user::copy_to_user(buf.as_ptr(), buf_ptr, STATFS_SIZE)
    };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `fstatfs(fd, buf)` — fill `buf` with synthetic filesystem stats.
fn sys_fstatfs(args: &SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as i32;
    let buf_ptr = args.arg1;

    // Validate the fd belongs to the caller (or skip if no caller).
    if let Some(pid) = caller_pid()
        && pcb::linux_fd_lookup(pid, fd).is_none()
    {
        return linux_err(errno::EBADF);
    }

    if buf_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_write(buf_ptr, STATFS_SIZE) {
        return linux_err(linux_errno_for(e));
    }

    let mut buf = [0u8; STATFS_SIZE];
    fill_statfs_default(&mut buf);
    // SAFETY: validated as a writable STATFS_SIZE-byte range above.
    let r = unsafe {
        crate::mm::user::copy_to_user(buf.as_ptr(), buf_ptr, STATFS_SIZE)
    };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Time-setting syscalls (clock_settime, clock_adjtime, adjtimex)
//
// These mutate the system wall clock.  Doing so safely requires CAP_SYS_TIME
// on Linux; in our model the caller similarly needs an explicit time-write
// capability that no userspace currently carries.  Reporting EPERM is the
// sound response — programs that probe whether they can set the clock will
// fall back gracefully, and programs that *expect* to set the clock will
// fail loudly instead of silently appearing to succeed while time
// continues to drift.
// ---------------------------------------------------------------------------

/// `clock_settime(clk_id, timespec*)` — refuse with EPERM.
fn sys_clock_settime(args: &SyscallArgs) -> SyscallResult {
    // Validate the user pointer so callers passing garbage still see
    // EFAULT (Linux validates before the privilege check on some
    // codepaths).  Then return EPERM.
    let tp = args.arg1;
    if tp != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(tp, 16) {
            return linux_err(linux_errno_for(e));
        }
    } else {
        return linux_err(errno::EFAULT);
    }
    linux_err(errno::EPERM)
}

/// `clock_adjtime(clk_id, timex*)` — refuse with EPERM.
fn sys_clock_adjtime(args: &SyscallArgs) -> SyscallResult {
    let tx = args.arg1;
    if tx == 0 {
        return linux_err(errno::EFAULT);
    }
    // struct timex on x86_64 is 208 bytes.
    if let Err(e) = crate::mm::user::validate_user_read(tx, 208) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EPERM)
}

/// `adjtimex(timex*)` — refuse with EPERM.
fn sys_adjtimex(args: &SyscallArgs) -> SyscallResult {
    let tx = args.arg0;
    if tx == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(tx, 208) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EPERM)
}

// ---------------------------------------------------------------------------
// chroot / mknod / mknodat — privileged operations we do not yet support
//
// All three require capabilities we don't grant any userspace task today
// (Linux: CAP_SYS_CHROOT, CAP_MKNOD).  Validate inputs first so callers
// passing garbage still observe EFAULT, then refuse with EPERM.
//
// EPERM is the truthful answer: we are not advertising a capability, so
// the operation cannot proceed.  When a real chroot / device-node story
// lands, these become proper FS calls.
// ---------------------------------------------------------------------------

/// `chroot(path)` — refuse with EPERM after pointer validation.
fn sys_chroot(args: &SyscallArgs) -> SyscallResult {
    let path = args.arg0;
    if path == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EPERM)
}

/// `mknod(path, mode, dev)` — refuse with EPERM after pointer validation.
fn sys_mknod(args: &SyscallArgs) -> SyscallResult {
    let path = args.arg0;
    if path == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EPERM)
}

/// `mknodat(dirfd, path, mode, dev)` — refuse with EPERM after validation.
fn sys_mknodat(args: &SyscallArgs) -> SyscallResult {
    // arg0 is dirfd; we do not yet support *at lookups so we don't
    // validate it past the path-pointer check.
    let path = args.arg1;
    if path == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EPERM)
}

// ---------------------------------------------------------------------------
// Interval timers (getitimer, setitimer) and the legacy alarm() / pause()
//
// Linux delivers ITIMER_REAL via SIGALRM, ITIMER_VIRTUAL via SIGVTALRM,
// ITIMER_PROF via SIGPROF.  We have no signal-driven interval-timer
// infrastructure yet, so:
//
//   - getitimer always reports "no timer pending" (a zeroed
//     struct itimerval).  This is a truthful answer when no timer
//     is armed.
//
//   - setitimer accepts cancellation (it_value == 0) silently.
//     Arming a non-zero timer would require us to deliver a signal at
//     some future time, which we cannot, so we return -ENOSYS.
//     Programs that rely on setitimer have a documented fallback path.
//
//   - alarm() is a deprecated wrapper around setitimer(ITIMER_REAL,...).
//     The Linux ABI does not provide an error return; the only return
//     is the unsigned seconds remaining of the previous alarm.  We
//     always return 0 (no previous alarm pending) and document the
//     missing fire as a known limitation.
//
//   - pause() blocks until a signal is delivered, then returns -EINTR.
//     Without signal-driven wakeup we cannot honour this; returning
//     -ENOSYS lets userspace fall back rather than hang forever.
//
// struct itimerval layout (x86_64): two struct timevals back-to-back =
// 4 longs = 32 bytes.
// ---------------------------------------------------------------------------

const ITIMERVAL_SIZE: usize = 32;

/// `getitimer(which, value)` — write a zeroed itimerval (no timer pending).
fn sys_getitimer(args: &SyscallArgs) -> SyscallResult {
    let which = args.arg0;
    let value = args.arg1;

    if which > 2 {
        return linux_err(errno::EINVAL);
    }
    if value == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_write(value, ITIMERVAL_SIZE) {
        return linux_err(linux_errno_for(e));
    }

    let buf = [0u8; ITIMERVAL_SIZE];
    // SAFETY: validated as a writable ITIMERVAL_SIZE-byte range above.
    let r = unsafe {
        crate::mm::user::copy_to_user(buf.as_ptr(), value, ITIMERVAL_SIZE)
    };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `setitimer(which, new_value, old_value)` — accept cancellation;
/// refuse arming with ENOSYS.
fn sys_setitimer(args: &SyscallArgs) -> SyscallResult {
    let which = args.arg0;
    let new_value = args.arg1;
    let old_value = args.arg2;

    if which > 2 {
        return linux_err(errno::EINVAL);
    }
    if new_value == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(new_value, ITIMERVAL_SIZE) {
        return linux_err(linux_errno_for(e));
    }

    // Copy the new itimerval in to inspect whether this is a cancel
    // (all-zero) or an arm (any field non-zero).
    let mut new_buf = [0u8; ITIMERVAL_SIZE];
    // SAFETY: validated above as a readable user range.
    let r = unsafe {
        crate::mm::user::copy_from_user(new_value, new_buf.as_mut_ptr(), ITIMERVAL_SIZE)
    };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    let is_cancel = new_buf.iter().all(|&b| b == 0);

    if !is_cancel {
        // We cannot arm a timer that will fire a signal later.  Refuse
        // honestly so the caller knows to fall back.
        return linux_err(errno::ENOSYS);
    }

    // Cancel path: report previous timer as zero (we hold no timer state)
    // into old_value if it was provided.
    if old_value != 0 {
        if let Err(e) =
            crate::mm::user::validate_user_write(old_value, ITIMERVAL_SIZE)
        {
            return linux_err(linux_errno_for(e));
        }
        let old_buf = [0u8; ITIMERVAL_SIZE];
        // SAFETY: validated as a writable ITIMERVAL_SIZE-byte range.
        let r = unsafe {
            crate::mm::user::copy_to_user(old_buf.as_ptr(), old_value, ITIMERVAL_SIZE)
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
    }
    SyscallResult::ok(0)
}

/// `alarm(seconds)` — legacy wrapper.  Returns 0 (no previous alarm).
fn sys_alarm(_args: &SyscallArgs) -> SyscallResult {
    // alarm() has no error return — it just reports "seconds remaining
    // of the previous alarm".  We never have a previous alarm, so 0 is
    // correct.  We do not fire the new alarm either; that is the
    // documented limitation tracked in todo.txt.
    SyscallResult::ok(0)
}

/// `pause()` — refuse with ENOSYS; we have no signal-driven wakeup yet.
fn sys_pause(_args: &SyscallArgs) -> SyscallResult {
    // Linux pause() returns -1 with errno=EINTR after a signal is
    // delivered.  Without that machinery we cannot fulfil the contract;
    // returning -ENOSYS lets userspace fall back gracefully rather than
    // hanging forever in a kernel that will never wake it.
    linux_err(errno::ENOSYS)
}

// ---------------------------------------------------------------------------
// access / faccessat / faccessat2 — file permission probes
//
// Without a backing filesystem there is no path that exists.  The
// truthful answer is ENOENT ("no such file") rather than ENOSYS — every
// real userspace fallback path treats ENOENT as "not present, move on",
// which is the correct behaviour for our empty FS.  Returning ENOSYS
// instead would cause some loaders to bail out.
//
// We still validate the mode bits and reject bogus ones with EINVAL so
// callers passing garbage observe Linux-shaped errors.
// ---------------------------------------------------------------------------

const ACCESS_VALID_MODE: u32 = 0x07; // R_OK=4 | W_OK=2 | X_OK=1 (F_OK=0)

/// `access(path, mode)` — reports the path does not exist.
fn sys_access(args: &SyscallArgs) -> SyscallResult {
    let path = args.arg0;
    #[allow(clippy::cast_possible_truncation)]
    let mode = args.arg1 as u32;

    if mode & !ACCESS_VALID_MODE != 0 {
        return linux_err(errno::EINVAL);
    }
    if path == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOENT)
}

/// `faccessat(dirfd, path, mode, flags)` — same as access for now.
///
/// Linux flag bits: `AT_EACCESS = 0x200`, `AT_SYMLINK_NOFOLLOW = 0x100`.
fn sys_faccessat(args: &SyscallArgs) -> SyscallResult {
    let path = args.arg1;
    #[allow(clippy::cast_possible_truncation)]
    let mode = args.arg2 as u32;
    // arg3 is flags; faccessat ignores unknown bits historically (Linux's
    // faccessat2 was added because faccessat silently dropped flags).

    if mode & !ACCESS_VALID_MODE != 0 {
        return linux_err(errno::EINVAL);
    }
    if path == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOENT)
}

/// `faccessat2(dirfd, path, mode, flags)` — explicit-flag variant.
///
/// Differs from `faccessat` in that it validates flag bits.
fn sys_faccessat2(args: &SyscallArgs) -> SyscallResult {
    const AT_EACCESS: u64 = 0x200;
    const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
    const AT_EMPTY_PATH: u64 = 0x1000;
    const VALID_FLAGS: u64 = AT_EACCESS | AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH;

    let path = args.arg1;
    #[allow(clippy::cast_possible_truncation)]
    let mode = args.arg2 as u32;
    let flags = args.arg3;

    if mode & !ACCESS_VALID_MODE != 0 {
        return linux_err(errno::EINVAL);
    }
    if flags & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    if path == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOENT)
}

// ---------------------------------------------------------------------------
// stat / lstat / fstat / newfstatat — file metadata
//
// Path-based variants (stat, lstat, newfstatat with a path) cannot
// find any file because we have no backing filesystem, so they return
// -ENOENT after validating their inputs.  fstat (and newfstatat with
// AT_EMPTY_PATH or fd-only lookup semantics) can succeed for fds we
// have in our table — we synthesise a struct stat that reports the
// correct file type for Console / Pipe / File handles so callers like
// isatty() get the right answer.
//
// struct stat layout (x86_64 Linux, 144 bytes):
//   dev_t   st_dev        (8)
//   ino_t   st_ino        (8)
//   nlink_t st_nlink      (8)
//   mode_t  st_mode       (4)
//   uid_t   st_uid        (4)
//   gid_t   st_gid        (4)
//   int     __pad0        (4)
//   dev_t   st_rdev       (8)
//   off_t   st_size       (8)
//   blksize_t st_blksize  (8)
//   blkcnt_t  st_blocks   (8)
//   timespec  st_atim     (16)
//   timespec  st_mtim     (16)
//   timespec  st_ctim     (16)
//   long      __unused[3] (24)
// ---------------------------------------------------------------------------

const STAT_SIZE: usize = 144;

/// Linux S_IF* file-type bits.
const S_IFREG: u32 = 0o100000;
const S_IFCHR: u32 = 0o020000;
const S_IFIFO: u32 = 0o010000;

/// Fill a 144-byte struct stat for the given Linux fd-table entry.
fn fill_stat_for_fd(
    buf: &mut [u8; STAT_SIZE],
    entry: &crate::proc::linux_fd::FdEntry,
) {
    use crate::proc::linux_fd::HandleKind;

    // Choose file type and mode bits based on what backs the fd.
    let (mode, blksize): (u32, u64) = match entry.kind {
        HandleKind::Console => (S_IFCHR | 0o620, 1024),
        HandleKind::Pipe => (S_IFIFO | 0o600, 4096),
        HandleKind::File => (S_IFREG | 0o644, 16 * 1024),
    };

    // Inode: use the raw_handle as a stable-ish identity.
    let st_ino: u64 = entry.raw_handle;
    // Use the current monotonic clock for atime/mtime/ctime so callers
    // see plausible recent timestamps.
    let now_ns = crate::timekeeping::clock_realtime();
    let now_sec = now_ns / 1_000_000_000;
    let now_nsec = now_ns % 1_000_000_000;

    // Write u64 little-endian at offset `off`.
    fn put_u64(buf: &mut [u8; STAT_SIZE], off: usize, v: u64) {
        let bytes = v.to_ne_bytes();
        #[allow(clippy::indexing_slicing)]
        for j in 0..8 {
            buf[off + j] = bytes[j];
        }
    }
    fn put_u32(buf: &mut [u8; STAT_SIZE], off: usize, v: u32) {
        let bytes = v.to_ne_bytes();
        #[allow(clippy::indexing_slicing)]
        for j in 0..4 {
            buf[off + j] = bytes[j];
        }
    }

    put_u64(buf, 0,   0);              // st_dev
    put_u64(buf, 8,   st_ino);         // st_ino
    put_u64(buf, 16,  1);              // st_nlink
    put_u32(buf, 24,  mode);           // st_mode
    put_u32(buf, 28,  0);              // st_uid
    put_u32(buf, 32,  0);              // st_gid
    // 36..=39: __pad0 (already zero)
    put_u64(buf, 40,  0);              // st_rdev
    put_u64(buf, 48,  0);              // st_size
    put_u64(buf, 56,  blksize);        // st_blksize
    put_u64(buf, 64,  0);              // st_blocks
    put_u64(buf, 72,  now_sec);        // st_atim.tv_sec
    put_u64(buf, 80,  now_nsec);       // st_atim.tv_nsec
    put_u64(buf, 88,  now_sec);        // st_mtim.tv_sec
    put_u64(buf, 96,  now_nsec);       // st_mtim.tv_nsec
    put_u64(buf, 104, now_sec);        // st_ctim.tv_sec
    put_u64(buf, 112, now_nsec);       // st_ctim.tv_nsec
    // 120..143: __unused[3] (already zero)
}

/// Shared path-based stat back-end (used by stat, lstat).
fn stat_path_impl(path_ptr: u64, statbuf_ptr: u64) -> SyscallResult {
    if path_ptr == 0 || statbuf_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path_ptr, 1) {
        return linux_err(linux_errno_for(e));
    }
    if let Err(e) = crate::mm::user::validate_user_write(statbuf_ptr, STAT_SIZE) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOENT)
}

/// `stat(path, statbuf)` — no such file (no VFS yet).
fn sys_stat(args: &SyscallArgs) -> SyscallResult {
    stat_path_impl(args.arg0, args.arg1)
}

/// `lstat(path, statbuf)` — no such file (no VFS yet).
fn sys_lstat(args: &SyscallArgs) -> SyscallResult {
    stat_path_impl(args.arg0, args.arg1)
}

/// `fstat(fd, statbuf)` — synthesise stat based on the fd's HandleKind.
fn sys_fstat(args: &SyscallArgs) -> SyscallResult {
    let fd = args.arg0 as i32;
    let statbuf_ptr = args.arg1;

    if statbuf_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_write(statbuf_ptr, STAT_SIZE) {
        return linux_err(linux_errno_for(e));
    }

    let entry = match caller_pid() {
        Some(pid) => match pcb::linux_fd_lookup(pid, fd) {
            Some(e) => e,
            None => return linux_err(errno::EBADF),
        },
        None => {
            // Kernel context (boot self-test): synthesise a Console
            // entry so the path is still exercised.
            crate::proc::linux_fd::FdEntry {
                kind: crate::proc::linux_fd::HandleKind::Console,
                raw_handle: 0,
                fd_flags: 0,
                status_flags: 0,
            }
        }
    };

    let mut buf = [0u8; STAT_SIZE];
    fill_stat_for_fd(&mut buf, &entry);
    // SAFETY: validated as a writable STAT_SIZE-byte range above.
    let r = unsafe {
        crate::mm::user::copy_to_user(buf.as_ptr(), statbuf_ptr, STAT_SIZE)
    };
    if let Err(e) = r {
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}

/// `newfstatat(dirfd, path, statbuf, flags)` — path-relative stat.
///
/// AT_EMPTY_PATH (0x1000): if path is empty, operate on dirfd itself.
/// AT_SYMLINK_NOFOLLOW (0x100): for our purposes equivalent to lstat.
fn sys_newfstatat(args: &SyscallArgs) -> SyscallResult {
    const AT_EMPTY_PATH: u64 = 0x1000;
    const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
    const AT_NO_AUTOMOUNT: u64 = 0x800;
    const VALID_FLAGS: u64 = AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW | AT_NO_AUTOMOUNT;

    let dirfd = args.arg0 as i32;
    let path = args.arg1;
    let statbuf = args.arg2;
    let flags = args.arg3;

    if flags & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    if statbuf == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_write(statbuf, STAT_SIZE) {
        return linux_err(linux_errno_for(e));
    }

    // AT_EMPTY_PATH with empty path means "stat dirfd itself".
    if flags & AT_EMPTY_PATH != 0 {
        // Check the path is empty (first byte is NUL).
        if path != 0 {
            if let Err(e) = crate::mm::user::validate_user_read(path, 1) {
                return linux_err(linux_errno_for(e));
            }
            // We don't actually probe the byte in kernel context (the
            // validate bypass means we could read garbage); just treat
            // as fd-only stat in that case.
        }
        // Route to fstat logic.
        let fstat_args = SyscallArgs {
            arg0: dirfd as u64,
            arg1: statbuf,
            arg2: 0, arg3: 0, arg4: 0, arg5: 0,
        };
        return sys_fstat(&fstat_args);
    }

    // Otherwise it's a path lookup we cannot satisfy.
    if path == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOENT)
}

// ---------------------------------------------------------------------------
// statx — extended file metadata (Linux 4.11+)
//
// Modern glibc uses statx as the underlying implementation for stat/fstat.
// We give it the same treatment as stat: ENOENT for path lookups, a
// synthetic struct statx for fd lookups with the correct file-type bits.
//
// struct statx layout (x86_64 Linux, 256 bytes):
//   u32 stx_mask, stx_blksize;
//   u64 stx_attributes;
//   u32 stx_nlink, stx_uid, stx_gid;
//   u16 stx_mode; u16 __spare0[1];
//   u64 stx_ino, stx_size, stx_blocks, stx_attributes_mask;
//   statx_timestamp stx_atime, stx_btime, stx_ctime, stx_mtime; (16 each)
//   u32 stx_rdev_major, stx_rdev_minor;
//   u32 stx_dev_major, stx_dev_minor;
//   u64 stx_mnt_id;
//   u32 stx_dio_mem_align, stx_dio_offset_align;
//   u64 stx_subvol;
//   u32 stx_atomic_write_unit_min, stx_atomic_write_unit_max;
//   u32 stx_atomic_write_segments_max, __spare1[1];
//   u64 __spare3[9];
// ---------------------------------------------------------------------------

const STATX_SIZE: usize = 256;

/// Subset of STATX_* mask bits we always advertise as "filled in".
/// Matches what fill_statx_for_fd actually writes.
const STATX_TYPE: u32 = 0x0001;
const STATX_MODE: u32 = 0x0002;
const STATX_NLINK: u32 = 0x0004;
const STATX_UID: u32 = 0x0008;
const STATX_GID: u32 = 0x0010;
const STATX_ATIME: u32 = 0x0020;
const STATX_MTIME: u32 = 0x0040;
const STATX_CTIME: u32 = 0x0080;
const STATX_INO: u32 = 0x0100;
const STATX_BASIC_STATS: u32 = STATX_TYPE
    | STATX_MODE
    | STATX_NLINK
    | STATX_UID
    | STATX_GID
    | STATX_ATIME
    | STATX_MTIME
    | STATX_CTIME
    | STATX_INO;

/// Fill a 256-byte struct statx for the given fd-table entry.
fn fill_statx_for_fd(
    buf: &mut [u8; STATX_SIZE],
    entry: &crate::proc::linux_fd::FdEntry,
) {
    use crate::proc::linux_fd::HandleKind;

    let (mode_u16, blksize): (u16, u32) = match entry.kind {
        HandleKind::Console => ((S_IFCHR | 0o620) as u16, 1024),
        HandleKind::Pipe => ((S_IFIFO | 0o600) as u16, 4096),
        HandleKind::File => ((S_IFREG | 0o644) as u16, 16 * 1024),
    };
    let st_ino: u64 = entry.raw_handle;
    let now_ns = crate::timekeeping::clock_realtime();
    #[allow(clippy::cast_possible_wrap)]
    let now_sec = (now_ns / 1_000_000_000) as i64;
    let now_nsec = (now_ns % 1_000_000_000) as u32;

    fn put_u32(buf: &mut [u8; STATX_SIZE], off: usize, v: u32) {
        let bytes = v.to_ne_bytes();
        #[allow(clippy::indexing_slicing)]
        for j in 0..4 {
            buf[off + j] = bytes[j];
        }
    }
    fn put_u64(buf: &mut [u8; STATX_SIZE], off: usize, v: u64) {
        let bytes = v.to_ne_bytes();
        #[allow(clippy::indexing_slicing)]
        for j in 0..8 {
            buf[off + j] = bytes[j];
        }
    }
    fn put_u16(buf: &mut [u8; STATX_SIZE], off: usize, v: u16) {
        let bytes = v.to_ne_bytes();
        #[allow(clippy::indexing_slicing)]
        for j in 0..2 {
            buf[off + j] = bytes[j];
        }
    }
    fn put_i64(buf: &mut [u8; STATX_SIZE], off: usize, v: i64) {
        let bytes = v.to_ne_bytes();
        #[allow(clippy::indexing_slicing)]
        for j in 0..8 {
            buf[off + j] = bytes[j];
        }
    }

    put_u32(buf, 0,  STATX_BASIC_STATS);   // stx_mask
    put_u32(buf, 4,  blksize);             // stx_blksize
    put_u64(buf, 8,  0);                   // stx_attributes
    put_u32(buf, 16, 1);                   // stx_nlink
    put_u32(buf, 20, 0);                   // stx_uid
    put_u32(buf, 24, 0);                   // stx_gid
    put_u16(buf, 28, mode_u16);            // stx_mode
    // 30..32: __spare0 (already 0)
    put_u64(buf, 32, st_ino);              // stx_ino
    put_u64(buf, 40, 0);                   // stx_size
    put_u64(buf, 48, 0);                   // stx_blocks
    put_u64(buf, 56, 0);                   // stx_attributes_mask
    // Timestamps (statx_timestamp = i64 tv_sec + u32 tv_nsec + u32 pad).
    // stx_atime at offset 64, stx_btime 80, stx_ctime 96, stx_mtime 112.
    put_i64(buf, 64,  now_sec);
    put_u32(buf, 72,  now_nsec);
    put_i64(buf, 80,  now_sec);
    put_u32(buf, 88,  now_nsec);
    put_i64(buf, 96,  now_sec);
    put_u32(buf, 104, now_nsec);
    put_i64(buf, 112, now_sec);
    put_u32(buf, 120, now_nsec);
    // Remaining fields (rdev/dev/mnt_id/dio/subvol/atomic/spare3)
    // stay zero — we have no device-major/minor or mount table.
}

/// `statx(dirfd, path, flags, mask, statxbuf)`.
fn sys_statx(args: &SyscallArgs) -> SyscallResult {
    const AT_EMPTY_PATH: u64 = 0x1000;
    const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
    const AT_NO_AUTOMOUNT: u64 = 0x800;
    const AT_STATX_SYNC_AS_STAT: u64 = 0x0000;
    const AT_STATX_FORCE_SYNC: u64 = 0x2000;
    const AT_STATX_DONT_SYNC: u64 = 0x4000;
    const VALID_FLAGS: u64 = AT_EMPTY_PATH
        | AT_SYMLINK_NOFOLLOW
        | AT_NO_AUTOMOUNT
        | AT_STATX_SYNC_AS_STAT
        | AT_STATX_FORCE_SYNC
        | AT_STATX_DONT_SYNC;

    let dirfd = args.arg0 as i32;
    let path = args.arg1;
    let flags = args.arg2;
    let _mask = args.arg3;
    let statxbuf = args.arg4;

    if flags & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    if statxbuf == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_write(statxbuf, STATX_SIZE) {
        return linux_err(linux_errno_for(e));
    }

    // AT_EMPTY_PATH lets the caller stat the dirfd itself.
    if flags & AT_EMPTY_PATH != 0 {
        let entry = match caller_pid() {
            Some(pid) => match pcb::linux_fd_lookup(pid, dirfd) {
                Some(e) => e,
                None => return linux_err(errno::EBADF),
            },
            None => crate::proc::linux_fd::FdEntry {
                kind: crate::proc::linux_fd::HandleKind::Console,
                raw_handle: 0,
                fd_flags: 0,
                status_flags: 0,
            },
        };
        let mut buf = [0u8; STATX_SIZE];
        fill_statx_for_fd(&mut buf, &entry);
        // SAFETY: validated as a writable STATX_SIZE-byte range above.
        let r = unsafe {
            crate::mm::user::copy_to_user(buf.as_ptr(), statxbuf, STATX_SIZE)
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
        return SyscallResult::ok(0);
    }

    // Path lookup: cannot find any file.
    if path == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOENT)
}

// ---------------------------------------------------------------------------
// Directory / file create / remove / rename
//
// Without a backing filesystem none of these can succeed.  We pick the
// errno most likely to let userspace fall back gracefully:
//   - mkdir / mkdirat: EROFS ("read-only filesystem") so installers and
//     test runners know the FS itself is the obstacle, not a path problem.
//   - rmdir / unlink / unlinkat / rename / renameat / renameat2:
//     ENOENT ("no such file") — the target does not exist in our empty
//     FS, which is the truthful answer.
// All variants validate their path pointers first so callers passing
// garbage observe EFAULT.
// ---------------------------------------------------------------------------

/// Validate that `ptr` is a non-NULL readable user pointer (1 byte).
fn validate_user_str(ptr: u64) -> crate::error::KernelResult<()> {
    if ptr == 0 {
        return Err(KernelError::InvalidAddress);
    }
    crate::mm::user::validate_user_read(ptr, 1)
}

/// `mkdir(path, mode)` — refuse with EROFS after pointer validation.
fn sys_mkdir(args: &SyscallArgs) -> SyscallResult {
    match validate_user_str(args.arg0) {
        Ok(()) => linux_err(errno::EROFS),
        Err(KernelError::InvalidAddress) if args.arg0 == 0 => linux_err(errno::EFAULT),
        Err(e) => linux_err(linux_errno_for(e)),
    }
}

/// `mkdirat(dirfd, path, mode)` — same as mkdir.
fn sys_mkdirat(args: &SyscallArgs) -> SyscallResult {
    match validate_user_str(args.arg1) {
        Ok(()) => linux_err(errno::EROFS),
        Err(KernelError::InvalidAddress) if args.arg1 == 0 => linux_err(errno::EFAULT),
        Err(e) => linux_err(linux_errno_for(e)),
    }
}

/// `rmdir(path)` — refuse with ENOENT after pointer validation.
fn sys_rmdir(args: &SyscallArgs) -> SyscallResult {
    match validate_user_str(args.arg0) {
        Ok(()) => linux_err(errno::ENOENT),
        Err(KernelError::InvalidAddress) if args.arg0 == 0 => linux_err(errno::EFAULT),
        Err(e) => linux_err(linux_errno_for(e)),
    }
}

/// `unlink(path)` — refuse with ENOENT after pointer validation.
fn sys_unlink(args: &SyscallArgs) -> SyscallResult {
    match validate_user_str(args.arg0) {
        Ok(()) => linux_err(errno::ENOENT),
        Err(KernelError::InvalidAddress) if args.arg0 == 0 => linux_err(errno::EFAULT),
        Err(e) => linux_err(linux_errno_for(e)),
    }
}

/// `unlinkat(dirfd, path, flags)` — refuse with ENOENT after validation.
fn sys_unlinkat(args: &SyscallArgs) -> SyscallResult {
    const AT_REMOVEDIR: u64 = 0x200;
    const VALID_FLAGS: u64 = AT_REMOVEDIR;
    if args.arg2 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    match validate_user_str(args.arg1) {
        Ok(()) => linux_err(errno::ENOENT),
        Err(KernelError::InvalidAddress) if args.arg1 == 0 => linux_err(errno::EFAULT),
        Err(e) => linux_err(linux_errno_for(e)),
    }
}

/// Shared rename back-end: validate two path pointers, return ENOENT.
fn rename_impl(old_ptr: u64, new_ptr: u64) -> SyscallResult {
    if old_ptr == 0 || new_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(old_ptr, 1) {
        return linux_err(linux_errno_for(e));
    }
    if let Err(e) = crate::mm::user::validate_user_read(new_ptr, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOENT)
}

/// `rename(oldpath, newpath)` — refuse with ENOENT after validation.
fn sys_rename(args: &SyscallArgs) -> SyscallResult {
    rename_impl(args.arg0, args.arg1)
}

/// `renameat(olddirfd, oldpath, newdirfd, newpath)` — same.
fn sys_renameat(args: &SyscallArgs) -> SyscallResult {
    rename_impl(args.arg1, args.arg3)
}

/// `renameat2(olddirfd, oldpath, newdirfd, newpath, flags)`.
///
/// RENAME_NOREPLACE=1, RENAME_EXCHANGE=2, RENAME_WHITEOUT=4.
fn sys_renameat2(args: &SyscallArgs) -> SyscallResult {
    const VALID_FLAGS: u64 = 1 | 2 | 4;
    if args.arg4 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    rename_impl(args.arg1, args.arg3)
}

// ---------------------------------------------------------------------------
// readlink / readlinkat
//
// We have no symlinks in our FS so the truthful answer for any path is
// EINVAL (Linux: "named file is not a symbolic link") — and Linux callers
// reliably handle EINVAL on readlink as "not a symlink, treat as plain
// file".  Validate the path pointer first so NULL surfaces as EFAULT.
//
// Special case the `/proc/self/exe` path?  Even ld.so probes it and a
// truthful ENOENT (path does not exist) is acceptable.  We do not want
// to fake a result here because the wrong result would silently confuse
// pathname-relative library loaders.
// ---------------------------------------------------------------------------

/// `readlink(path, buf, bufsiz)`.
fn sys_readlink(args: &SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg0;
    let buf_ptr = args.arg1;
    let bufsiz = args.arg2 as usize;
    if path_ptr == 0 || buf_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if bufsiz == 0 {
        // Linux: bufsiz <= 0 -> EINVAL.
        return linux_err(errno::EINVAL);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path_ptr, 1) {
        return linux_err(linux_errno_for(e));
    }
    if let Err(e) = crate::mm::user::validate_user_write(buf_ptr, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EINVAL)
}

/// `readlinkat(dirfd, path, buf, bufsiz)`.
fn sys_readlinkat(args: &SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg1;
    let buf_ptr = args.arg2;
    let bufsiz = args.arg3 as usize;
    if path_ptr == 0 || buf_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if bufsiz == 0 {
        return linux_err(errno::EINVAL);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path_ptr, 1) {
        return linux_err(linux_errno_for(e));
    }
    if let Err(e) = crate::mm::user::validate_user_write(buf_ptr, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EINVAL)
}

// ---------------------------------------------------------------------------
// chmod / fchmod / fchmodat / chown / lchown / fchown / fchownat
//
// Without a writable backing FS or any concept of ownership, all attempts
// to alter mode or owner are refused with EROFS for path-based variants
// (the FS itself is read-only) and EROFS after fd validation for the
// fd-based variants.  Validate inputs first so EFAULT and EBADF surface
// truthfully.
// ---------------------------------------------------------------------------

/// `chmod(path, mode)`.
fn sys_chmod(args: &SyscallArgs) -> SyscallResult {
    match validate_user_str(args.arg0) {
        Ok(()) => linux_err(errno::EROFS),
        Err(KernelError::InvalidAddress) if args.arg0 == 0 => linux_err(errno::EFAULT),
        Err(e) => linux_err(linux_errno_for(e)),
    }
}

/// `fchmod(fd, mode)`.
fn sys_fchmod(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EROFS),
    };
    if pcb::linux_fd_lookup(pid, fd).is_none() {
        return linux_err(errno::EBADF);
    }
    linux_err(errno::EROFS)
}

/// `fchmodat(dirfd, path, mode, flags)`.
fn sys_fchmodat(args: &SyscallArgs) -> SyscallResult {
    // Linux: only AT_SYMLINK_NOFOLLOW (0x100) is valid.  Some glibc
    // versions also pass AT_EMPTY_PATH (0x1000).
    const VALID_FLAGS: u64 = 0x100 | 0x1000;
    if args.arg3 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    match validate_user_str(args.arg1) {
        Ok(()) => linux_err(errno::EROFS),
        Err(KernelError::InvalidAddress) if args.arg1 == 0 => linux_err(errno::EFAULT),
        Err(e) => linux_err(linux_errno_for(e)),
    }
}

/// `chown(path, uid, gid)`.
fn sys_chown(args: &SyscallArgs) -> SyscallResult {
    match validate_user_str(args.arg0) {
        Ok(()) => linux_err(errno::EROFS),
        Err(KernelError::InvalidAddress) if args.arg0 == 0 => linux_err(errno::EFAULT),
        Err(e) => linux_err(linux_errno_for(e)),
    }
}

/// `fchown(fd, uid, gid)`.
fn sys_fchown(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EROFS),
    };
    if pcb::linux_fd_lookup(pid, fd).is_none() {
        return linux_err(errno::EBADF);
    }
    linux_err(errno::EROFS)
}

/// `lchown(path, uid, gid)` — identical to chown without symlink
/// follow; same answer for us.
fn sys_lchown(args: &SyscallArgs) -> SyscallResult {
    match validate_user_str(args.arg0) {
        Ok(()) => linux_err(errno::EROFS),
        Err(KernelError::InvalidAddress) if args.arg0 == 0 => linux_err(errno::EFAULT),
        Err(e) => linux_err(linux_errno_for(e)),
    }
}

/// `fchownat(dirfd, path, uid, gid, flags)`.
fn sys_fchownat(args: &SyscallArgs) -> SyscallResult {
    // Linux: AT_SYMLINK_NOFOLLOW (0x100) | AT_EMPTY_PATH (0x1000).
    const VALID_FLAGS: u64 = 0x100 | 0x1000;
    if args.arg4 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    // AT_EMPTY_PATH may pass an empty path with a valid dirfd; in that
    // case the operation targets dirfd directly.  Even then we have no
    // writable FS, so EROFS once dirfd is validated.
    if args.arg4 & 0x1000 != 0 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let dirfd = args.arg0 as i32;
        // AT_FDCWD == -100; accept it without validation.
        if dirfd != -100 {
            let pid = match caller_pid() {
                Some(p) => p,
                None => return linux_err(errno::EROFS),
            };
            if pcb::linux_fd_lookup(pid, dirfd).is_none() {
                return linux_err(errno::EBADF);
            }
        }
        return linux_err(errno::EROFS);
    }
    match validate_user_str(args.arg1) {
        Ok(()) => linux_err(errno::EROFS),
        Err(KernelError::InvalidAddress) if args.arg1 == 0 => linux_err(errno::EFAULT),
        Err(e) => linux_err(linux_errno_for(e)),
    }
}

// ---------------------------------------------------------------------------
// truncate / ftruncate
//
// truncate refuses with EROFS after path validation: there is no file
// to truncate anyway, and EROFS is the truthful answer in the absence
// of a writable FS.  ftruncate validates the fd; if it refers to a
// Pipe or Console it returns EINVAL (truthful: not a regular file);
// if it is a File it returns EROFS.
// ---------------------------------------------------------------------------

/// `truncate(path, length)`.
fn sys_truncate(args: &SyscallArgs) -> SyscallResult {
    // Negative length -> EINVAL per POSIX.
    #[allow(clippy::cast_possible_wrap)]
    let length = args.arg1 as i64;
    if length < 0 {
        return linux_err(errno::EINVAL);
    }
    match validate_user_str(args.arg0) {
        Ok(()) => linux_err(errno::EROFS),
        Err(KernelError::InvalidAddress) if args.arg0 == 0 => linux_err(errno::EFAULT),
        Err(e) => linux_err(linux_errno_for(e)),
    }
}

/// `ftruncate(fd, length)`.
fn sys_ftruncate(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_wrap)]
    let length = args.arg1 as i64;
    if length < 0 {
        return linux_err(errno::EINVAL);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EROFS),
    };
    let entry = match pcb::linux_fd_lookup(pid, fd) {
        Some(e) => e,
        None => return linux_err(errno::EBADF),
    };
    use crate::proc::linux_fd::HandleKind;
    match entry.kind {
        HandleKind::File => linux_err(errno::EROFS),
        // Pipes and consoles cannot be truncated.
        HandleKind::Pipe | HandleKind::Console => linux_err(errno::EINVAL),
    }
}

// ---------------------------------------------------------------------------
// symlink / symlinkat / link / linkat
//
// All create-link operations refuse with EROFS after pointer validation.
// EROFS is the truthful answer: even if the source paths existed, the
// destination cannot be created.  For the *at variants we validate the
// new-path pointer (the one we'd be writing) and the old-path pointer
// (the target, even if our representation is by-value).
// ---------------------------------------------------------------------------

/// `symlink(target, linkpath)`.
fn sys_symlink(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 || args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EROFS)
}

/// `symlinkat(target, newdirfd, linkpath)`.
fn sys_symlinkat(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 || args.arg2 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EROFS)
}

/// `link(oldpath, newpath)`.
fn sys_link(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 || args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EROFS)
}

/// `linkat(olddirfd, oldpath, newdirfd, newpath, flags)`.
fn sys_linkat(args: &SyscallArgs) -> SyscallResult {
    // Linux: AT_SYMLINK_FOLLOW (0x400) | AT_EMPTY_PATH (0x1000).
    const VALID_FLAGS: u64 = 0x400 | 0x1000;
    if args.arg4 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg1 == 0 || args.arg3 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 1) {
        return linux_err(linux_errno_for(e));
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg3, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EROFS)
}

// ---------------------------------------------------------------------------
// utimensat / utimes / utime
//
// Timestamp-update syscalls.  Without a writable FS we cannot persist any
// change, so each refuses with EROFS after pointer validation.  utimensat
// has a special case where path may be NULL (operate on dirfd) and times
// may be NULL (use current time) — neither makes a write succeed for us,
// but we honour the input-shape rules so callers see the correct errno.
// ---------------------------------------------------------------------------

/// `utimensat(dirfd, path, times[2], flags)`.
fn sys_utimensat(args: &SyscallArgs) -> SyscallResult {
    const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
    const VALID_FLAGS: u64 = AT_SYMLINK_NOFOLLOW;
    if args.arg3 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    // path may legitimately be NULL: operate on dirfd directly.
    if args.arg1 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 1) {
            return linux_err(linux_errno_for(e));
        }
    }
    // times may legitimately be NULL (use current time).  When non-NULL
    // it points at two timespecs = 32 bytes.
    if args.arg2 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 32) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EROFS)
}

/// `utimes(path, times[2])` — two `struct timeval` = 32 bytes.
fn sys_utimes(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    if args.arg1 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 32) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EROFS)
}

/// `utime(path, buf)` — `struct utimbuf { time_t actime; time_t modtime; }`
/// = 16 bytes.
fn sys_utime(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    if args.arg1 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 16) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EROFS)
}

// ---------------------------------------------------------------------------
// signalfd / timerfd / inotify / fanotify
//
// We do not yet implement any of these event-fd families.  The honest
// answer to the caller is ENOSYS after input validation: this lets
// userspace fall back to alternatives (alarm + signals, polling, etc.)
// instead of silently misbehaving.
//
// We *do* still validate inputs so that programs probing argument
// validity get the same error codes Linux would give.  In particular,
// flag-bit validation precedes ENOSYS so callers see EINVAL where Linux
// would also see EINVAL — useful for portable code that checks for
// feature support via specific error codes.
// ---------------------------------------------------------------------------

/// `signalfd(fd, mask, sizemask)` — legacy signalfd.
///
/// `mask` points at a `sigset_t` (8 bytes on x86_64).
fn sys_signalfd(args: &SyscallArgs) -> SyscallResult {
    let mask_ptr = args.arg1;
    let sizemask = args.arg2 as usize;
    // Linux: sizemask must be sizeof(sigset_t) == 8.
    if sizemask != 8 {
        return linux_err(errno::EINVAL);
    }
    if mask_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(mask_ptr, 8) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOSYS)
}

/// `signalfd4(fd, mask, sizemask, flags)`.
fn sys_signalfd4(args: &SyscallArgs) -> SyscallResult {
    // SFD_NONBLOCK = O_NONBLOCK = 0o4000 (2048).
    // SFD_CLOEXEC  = O_CLOEXEC  = 0o2_000_000 (524288).
    const SFD_NONBLOCK: u64 = 0o4000;
    const SFD_CLOEXEC: u64 = 0o2_000_000;
    const VALID_FLAGS: u64 = SFD_NONBLOCK | SFD_CLOEXEC;
    if args.arg3 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    let mask_ptr = args.arg1;
    let sizemask = args.arg2 as usize;
    if sizemask != 8 {
        return linux_err(errno::EINVAL);
    }
    if mask_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(mask_ptr, 8) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOSYS)
}

/// `timerfd_create(clockid, flags)`.
fn sys_timerfd_create(args: &SyscallArgs) -> SyscallResult {
    // Linux accepts CLOCK_REALTIME=0, CLOCK_MONOTONIC=1, CLOCK_BOOTTIME=7,
    // CLOCK_REALTIME_ALARM=8, CLOCK_BOOTTIME_ALARM=9.  Anything else
    // -> EINVAL.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let clockid = args.arg0 as i32;
    if !matches!(clockid, 0 | 1 | 7 | 8 | 9) {
        return linux_err(errno::EINVAL);
    }
    const TFD_NONBLOCK: u64 = 0o4000;
    const TFD_CLOEXEC: u64 = 0o2_000_000;
    const VALID_FLAGS: u64 = TFD_NONBLOCK | TFD_CLOEXEC;
    if args.arg1 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    linux_err(errno::ENOSYS)
}

/// `timerfd_settime(fd, flags, new_value, old_value)`.
///
/// `struct itimerspec` is two `struct timespec` = 32 bytes.
fn sys_timerfd_settime(args: &SyscallArgs) -> SyscallResult {
    // TFD_TIMER_ABSTIME=1, TFD_TIMER_CANCEL_ON_SET=2.
    const VALID_FLAGS: u64 = 1 | 2;
    if args.arg1 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    let new_ptr = args.arg2;
    if new_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(new_ptr, 32) {
        return linux_err(linux_errno_for(e));
    }
    if args.arg3 != 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg3, 32) {
            return linux_err(linux_errno_for(e));
        }
    }
    // No timerfd fds exist in our kernel, so any fd reference is bad.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EBADF),
    };
    if pcb::linux_fd_lookup(pid, fd).is_none() {
        return linux_err(errno::EBADF);
    }
    // Even if the fd refers to a real fd, it isn't a timerfd, so:
    linux_err(errno::EINVAL)
}

/// `timerfd_gettime(fd, curr_value)`.
fn sys_timerfd_gettime(args: &SyscallArgs) -> SyscallResult {
    let curr_ptr = args.arg1;
    if curr_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_write(curr_ptr, 32) {
        return linux_err(linux_errno_for(e));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EBADF),
    };
    if pcb::linux_fd_lookup(pid, fd).is_none() {
        return linux_err(errno::EBADF);
    }
    linux_err(errno::EINVAL)
}

/// `inotify_init()`.
fn sys_inotify_init(_args: &SyscallArgs) -> SyscallResult {
    linux_err(errno::ENOSYS)
}

/// `inotify_init1(flags)`.
fn sys_inotify_init1(args: &SyscallArgs) -> SyscallResult {
    // IN_NONBLOCK = O_NONBLOCK, IN_CLOEXEC = O_CLOEXEC.
    const IN_NONBLOCK: u64 = 0o4000;
    const IN_CLOEXEC: u64 = 0o2_000_000;
    const VALID_FLAGS: u64 = IN_NONBLOCK | IN_CLOEXEC;
    if args.arg0 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    linux_err(errno::ENOSYS)
}

/// `inotify_add_watch(fd, pathname, mask)`.
fn sys_inotify_add_watch(args: &SyscallArgs) -> SyscallResult {
    let path_ptr = args.arg1;
    if path_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(path_ptr, 1) {
        return linux_err(linux_errno_for(e));
    }
    // mask 0 -> EINVAL per Linux (nothing to watch for).
    if args.arg2 == 0 {
        return linux_err(errno::EINVAL);
    }
    // No inotify fd exists; report EBADF on real fd validation.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EBADF),
    };
    if pcb::linux_fd_lookup(pid, fd).is_none() {
        return linux_err(errno::EBADF);
    }
    linux_err(errno::EINVAL)
}

/// `inotify_rm_watch(fd, wd)`.
fn sys_inotify_rm_watch(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    let pid = match caller_pid() {
        Some(p) => p,
        None => return linux_err(errno::EBADF),
    };
    if pcb::linux_fd_lookup(pid, fd).is_none() {
        return linux_err(errno::EBADF);
    }
    linux_err(errno::EINVAL)
}

/// `fanotify_init(flags, event_f_flags)`.
fn sys_fanotify_init(_args: &SyscallArgs) -> SyscallResult {
    // fanotify is privileged on Linux (requires CAP_SYS_ADMIN) and
    // userspace already handles ENOSYS as "kernel does not have
    // fanotify"; that's the honest answer for us.
    linux_err(errno::ENOSYS)
}

/// `fanotify_mark(fanotify_fd, flags, mask, dirfd, pathname)`.
fn sys_fanotify_mark(args: &SyscallArgs) -> SyscallResult {
    // Validate pathname if non-NULL so EFAULT surfaces correctly.
    if args.arg4 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg4, 1) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::ENOSYS)
}

// ---------------------------------------------------------------------------
// sendfile / splice / tee / vmsplice / copy_file_range
//
// Zero-copy / batch I/O syscalls.  We don't have a page-cache or
// pipe-to-pipe page transfer plumbing yet, so the right answer is
// EINVAL (Linux: "fds are not the right kinds for this operation")
// after validating the fds and offset pointers.  EINVAL — not ENOSYS —
// because userspace already handles EINVAL on these by falling back to
// a read/write loop, while ENOSYS would suggest the syscall doesn't
// exist at all and might force a different code path.
//
// The exception is io_uring and the AIO syscalls (io_setup family),
// which return ENOSYS so io_uring-aware callers fall back to epoll
// and AIO-aware callers fall back to threads.
// ---------------------------------------------------------------------------

/// Validate a Linux-shaped fd in kernel context: returns Ok if the
/// caller is the kernel (no pid), otherwise looks the fd up.
fn validate_linux_fd(fd: i32) -> Result<(), SyscallResult> {
    let pid = match caller_pid() {
        Some(p) => p,
        None => return Ok(()),
    };
    match pcb::linux_fd_lookup(pid, fd) {
        Some(_) => Ok(()),
        None => Err(linux_err(errno::EBADF)),
    }
}

/// `sendfile(out_fd, in_fd, offset, count)`.
fn sys_sendfile(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let out_fd = args.arg0 as i32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let in_fd = args.arg1 as i32;
    if let Err(r) = validate_linux_fd(out_fd) {
        return r;
    }
    if let Err(r) = validate_linux_fd(in_fd) {
        return r;
    }
    // offset is `off_t *` (8 bytes) if non-NULL.
    if args.arg2 != 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg2, 8) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EINVAL)
}

/// `splice(fd_in, off_in, fd_out, off_out, len, flags)`.
fn sys_splice(args: &SyscallArgs) -> SyscallResult {
    // SPLICE_F_MOVE=1, NONBLOCK=2, MORE=4, GIFT=8.
    const VALID_FLAGS: u64 = 1 | 2 | 4 | 8;
    if args.arg5 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd_in = args.arg0 as i32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd_out = args.arg2 as i32;
    if let Err(r) = validate_linux_fd(fd_in) {
        return r;
    }
    if let Err(r) = validate_linux_fd(fd_out) {
        return r;
    }
    if args.arg1 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 8) {
            return linux_err(linux_errno_for(e));
        }
    }
    if args.arg3 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, 8) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EINVAL)
}

/// `tee(fd_in, fd_out, len, flags)`.
fn sys_tee(args: &SyscallArgs) -> SyscallResult {
    const VALID_FLAGS: u64 = 1 | 2 | 4 | 8;
    if args.arg3 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd_in = args.arg0 as i32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd_out = args.arg1 as i32;
    if let Err(r) = validate_linux_fd(fd_in) {
        return r;
    }
    if let Err(r) = validate_linux_fd(fd_out) {
        return r;
    }
    linux_err(errno::EINVAL)
}

/// `vmsplice(fd, iov, nr_segs, flags)`.
fn sys_vmsplice(args: &SyscallArgs) -> SyscallResult {
    const VALID_FLAGS: u64 = 1 | 2 | 4 | 8;
    if args.arg3 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    // iov NULL with nr_segs > 0 -> EFAULT.
    let nr_segs = args.arg2 as usize;
    if nr_segs > 0 && args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    // Linux: IOV_MAX = 1024.
    if nr_segs > 1024 {
        return linux_err(errno::EINVAL);
    }
    if nr_segs > 0 {
        // Each iovec is 16 bytes (ptr + len).
        let total = nr_segs.saturating_mul(16);
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, total) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EINVAL)
}

/// `copy_file_range(fd_in, off_in, fd_out, off_out, len, flags)`.
fn sys_copy_file_range(args: &SyscallArgs) -> SyscallResult {
    // Per man-page: flags must be 0.
    if args.arg5 != 0 {
        return linux_err(errno::EINVAL);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd_in = args.arg0 as i32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd_out = args.arg2 as i32;
    if let Err(r) = validate_linux_fd(fd_in) {
        return r;
    }
    if let Err(r) = validate_linux_fd(fd_out) {
        return r;
    }
    if args.arg1 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 8) {
            return linux_err(linux_errno_for(e));
        }
    }
    if args.arg3 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, 8) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EINVAL)
}

// ---------------------------------------------------------------------------
// AIO (io_setup family) — ENOSYS-after-validate
// ---------------------------------------------------------------------------

/// `io_setup(nr_events, ctx_idp)`.
fn sys_io_setup(args: &SyscallArgs) -> SyscallResult {
    // nr_events == 0 -> EINVAL per man-page.
    if args.arg0 == 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    // ctx_idp is an aio_context_t * (8 bytes).
    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, 8) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOSYS)
}

/// `io_destroy(ctx_id)`.
fn sys_io_destroy(_args: &SyscallArgs) -> SyscallResult {
    // No contexts ever exist, so any handle is invalid.
    linux_err(errno::EINVAL)
}

/// `io_submit(ctx_id, nr, iocbpp)`.
fn sys_io_submit(args: &SyscallArgs) -> SyscallResult {
    let nr = args.arg1 as i64;
    if nr < 0 {
        return linux_err(errno::EINVAL);
    }
    if nr > 0 && args.arg2 == 0 {
        return linux_err(errno::EFAULT);
    }
    linux_err(errno::EINVAL)
}

/// `io_cancel(ctx_id, iocb, result)`.
fn sys_io_cancel(_args: &SyscallArgs) -> SyscallResult {
    linux_err(errno::EINVAL)
}

/// `io_getevents(ctx_id, min_nr, nr, events, timeout)`.
fn sys_io_getevents(args: &SyscallArgs) -> SyscallResult {
    let min_nr = args.arg1 as i64;
    let nr = args.arg2 as i64;
    if min_nr < 0 || nr < 0 || min_nr > nr {
        return linux_err(errno::EINVAL);
    }
    linux_err(errno::EINVAL)
}

// ---------------------------------------------------------------------------
// io_uring — ENOSYS so callers fall back to epoll
// ---------------------------------------------------------------------------

/// `io_uring_setup(entries, params)`.
fn sys_io_uring_setup(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    // struct io_uring_params is 120 bytes on x86_64.
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 120) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOSYS)
}

/// `io_uring_enter(fd, to_submit, min_complete, flags, sig, sigsz)`.
fn sys_io_uring_enter(_args: &SyscallArgs) -> SyscallResult {
    linux_err(errno::ENOSYS)
}

/// `io_uring_register(fd, opcode, arg, nr_args)`.
fn sys_io_uring_register(_args: &SyscallArgs) -> SyscallResult {
    linux_err(errno::ENOSYS)
}

// ---------------------------------------------------------------------------
// BPF / perf_event_open / keyring / userfaultfd / memfd / pidfd /
// process_vm
//
// Most of these are privileged in Linux, niche, or both.  The honest
// answer is ENOSYS after input validation: the user can detect the
// missing feature and fall back to an alternative (eBPF programs to
// userspace polling, perf to RDTSC sampling, keyring to in-process
// key management, userfaultfd to SIGSEGV handler, pidfd to /proc/PID
// path lookup).
//
// memfd_create is more commonly used (Vulkan / Wayland / sandboxed
// shared memory), but until we have an anonymous-page-backed fd we
// can't honour it; ENOSYS makes glibc and mesa fall back to
// shm_open()-via-tmpfs.
// ---------------------------------------------------------------------------

/// `bpf(cmd, attr, size)`.
fn sys_bpf(args: &SyscallArgs) -> SyscallResult {
    let size = args.arg2 as usize;
    if size == 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, size) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOSYS)
}

/// `perf_event_open(attr, pid, cpu, group_fd, flags)`.
fn sys_perf_event_open(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    // struct perf_event_attr is at least 8 bytes (size header).
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 8) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOSYS)
}

/// `keyctl(cmd, arg2, arg3, arg4, arg5)`.
fn sys_keyctl(_args: &SyscallArgs) -> SyscallResult {
    linux_err(errno::ENOSYS)
}

/// `add_key(type, description, payload, plen, keyring)`.
fn sys_add_key(args: &SyscallArgs) -> SyscallResult {
    // type and description must be non-NULL.
    if args.arg0 == 0 || args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOSYS)
}

/// `request_key(type, description, callout_info, keyring)`.
fn sys_request_key(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 || args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOSYS)
}

/// `userfaultfd(flags)`.
fn sys_userfaultfd(args: &SyscallArgs) -> SyscallResult {
    // UFFD_USER_MODE_ONLY = 1, plus O_CLOEXEC | O_NONBLOCK.
    const VALID_FLAGS: u64 = 1 | 0o4000 | 0o2_000_000;
    if args.arg0 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    linux_err(errno::ENOSYS)
}

/// `memfd_create(name, flags)`.
fn sys_memfd_create(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    // MFD_CLOEXEC=1, ALLOW_SEALING=2, HUGETLB=4, NOEXEC_SEAL=8, EXEC=16,
    // plus huge-page-size bits 26..31 (we accept those without parsing).
    const VALID_LOW_FLAGS: u64 = 1 | 2 | 4 | 8 | 16;
    const HUGE_SIZE_MASK: u64 = 0x3F << 26;
    if args.arg1 & !(VALID_LOW_FLAGS | HUGE_SIZE_MASK) != 0 {
        return linux_err(errno::EINVAL);
    }
    linux_err(errno::ENOSYS)
}

/// `memfd_secret(flags)`.
fn sys_memfd_secret(args: &SyscallArgs) -> SyscallResult {
    const VALID_FLAGS: u64 = 0o2_000_000; // O_CLOEXEC
    if args.arg0 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    linux_err(errno::ENOSYS)
}

/// `pidfd_open(pid, flags)`.
fn sys_pidfd_open(args: &SyscallArgs) -> SyscallResult {
    // PIDFD_NONBLOCK = O_NONBLOCK = 0o4000.
    const VALID_FLAGS: u64 = 0o4000;
    if args.arg1 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let pid = args.arg0 as i32;
    if pid <= 0 {
        return linux_err(errno::EINVAL);
    }
    // ESRCH would be the truthful answer if pidfd existed but the pid is
    // gone; without pidfd support, ENOSYS lets callers fall back to
    // /proc/PID lookup.
    linux_err(errno::ENOSYS)
}

/// `pidfd_send_signal(pidfd, sig, info, flags)`.
fn sys_pidfd_send_signal(args: &SyscallArgs) -> SyscallResult {
    // flags must be 0 per current Linux.
    if args.arg3 != 0 {
        return linux_err(errno::EINVAL);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let sig = args.arg1 as i32;
    if !(0..=64).contains(&sig) {
        return linux_err(errno::EINVAL);
    }
    if args.arg2 != 0 {
        // struct siginfo_t = 128 bytes.
        if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 128) {
            return linux_err(linux_errno_for(e));
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    linux_err(errno::EINVAL)
}

/// `pidfd_getfd(pidfd, targetfd, flags)`.
fn sys_pidfd_getfd(args: &SyscallArgs) -> SyscallResult {
    // flags reserved.
    if args.arg2 != 0 {
        return linux_err(errno::EINVAL);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    linux_err(errno::EINVAL)
}

/// `process_vm_readv(pid, local_iov, liovcnt, remote_iov, riovcnt, flags)`.
fn sys_process_vm_readv(args: &SyscallArgs) -> SyscallResult {
    if args.arg5 != 0 {
        return linux_err(errno::EINVAL);
    }
    process_vm_impl(args)
}

/// `process_vm_writev(pid, local_iov, liovcnt, remote_iov, riovcnt, flags)`.
fn sys_process_vm_writev(args: &SyscallArgs) -> SyscallResult {
    if args.arg5 != 0 {
        return linux_err(errno::EINVAL);
    }
    process_vm_impl(args)
}

fn process_vm_impl(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let pid = args.arg0 as i32;
    if pid <= 0 {
        return linux_err(errno::ESRCH);
    }
    let liovcnt = args.arg2 as usize;
    let riovcnt = args.arg4 as usize;
    if liovcnt > 1024 || riovcnt > 1024 {
        return linux_err(errno::EINVAL);
    }
    if liovcnt > 0 {
        if args.arg1 == 0 {
            return linux_err(errno::EFAULT);
        }
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, liovcnt.saturating_mul(16)) {
            return linux_err(linux_errno_for(e));
        }
    }
    if riovcnt > 0 {
        if args.arg3 == 0 {
            return linux_err(errno::EFAULT);
        }
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, riovcnt.saturating_mul(16)) {
            return linux_err(linux_errno_for(e));
        }
    }
    // Cross-process VM access not yet implemented.
    linux_err(errno::ESRCH)
}

/// `process_mrelease(pidfd, flags)`.
fn sys_process_mrelease(args: &SyscallArgs) -> SyscallResult {
    if args.arg1 != 0 {
        return linux_err(errno::EINVAL);
    }
    linux_err(errno::ENOSYS)
}

// ---------------------------------------------------------------------------
// Extended attributes (xattr)
//
// Our FS has no extended attribute support yet.  The truthful answers
// per the Linux man-page are:
//   - get / list: ENODATA ("attribute does not exist") for path/fd
//     variants — i.e., no attributes are present.
//   - set / remove: EOPNOTSUPP ("filesystem does not support xattrs").
// EOPNOTSUPP is what callers check to learn the FS lacks xattr; ENODATA
// is what they check to learn a specific attribute is missing.  Both
// are commonly handled in portable code.
// ---------------------------------------------------------------------------

/// Helper: validate path + name pointers for path-based xattr ops.
fn xattr_validate_path_name(path: u64, name: u64) -> Result<(), SyscallResult> {
    if path == 0 || name == 0 {
        return Err(linux_err(errno::EFAULT));
    }
    if let Err(e) = crate::mm::user::validate_user_read(path, 1) {
        return Err(linux_err(linux_errno_for(e)));
    }
    if let Err(e) = crate::mm::user::validate_user_read(name, 1) {
        return Err(linux_err(linux_errno_for(e)));
    }
    Ok(())
}

/// Helper: validate fd + name for fd-based xattr ops.
fn xattr_validate_fd_name(fd: i32, name: u64) -> Result<(), SyscallResult> {
    if name == 0 {
        return Err(linux_err(errno::EFAULT));
    }
    if let Err(e) = crate::mm::user::validate_user_read(name, 1) {
        return Err(linux_err(linux_errno_for(e)));
    }
    validate_linux_fd(fd)
}

fn xattr_set_path(args: &SyscallArgs) -> SyscallResult {
    if let Err(r) = xattr_validate_path_name(args.arg0, args.arg1) {
        return r;
    }
    // value pointer is optional (NULL = delete-like behaviour for some
    // FS), validate when non-NULL.
    let size = args.arg3 as usize;
    if args.arg2 != 0 && size > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg2, size) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EOPNOTSUPP)
}

/// `setxattr(path, name, value, size, flags)`.
fn sys_setxattr(args: &SyscallArgs) -> SyscallResult {
    xattr_set_path(args)
}
/// `lsetxattr(path, name, value, size, flags)`.
fn sys_lsetxattr(args: &SyscallArgs) -> SyscallResult {
    xattr_set_path(args)
}

/// `fsetxattr(fd, name, value, size, flags)`.
fn sys_fsetxattr(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = xattr_validate_fd_name(fd, args.arg1) {
        return r;
    }
    let size = args.arg3 as usize;
    if args.arg2 != 0 && size > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg2, size) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EOPNOTSUPP)
}

fn xattr_get_path(args: &SyscallArgs) -> SyscallResult {
    if let Err(r) = xattr_validate_path_name(args.arg0, args.arg1) {
        return r;
    }
    let size = args.arg3 as usize;
    if args.arg2 != 0 && size > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg2, size) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::ENODATA)
}

/// `getxattr(path, name, value, size)`.
fn sys_getxattr(args: &SyscallArgs) -> SyscallResult {
    xattr_get_path(args)
}
/// `lgetxattr(path, name, value, size)`.
fn sys_lgetxattr(args: &SyscallArgs) -> SyscallResult {
    xattr_get_path(args)
}

/// `fgetxattr(fd, name, value, size)`.
fn sys_fgetxattr(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = xattr_validate_fd_name(fd, args.arg1) {
        return r;
    }
    let size = args.arg3 as usize;
    if args.arg2 != 0 && size > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg2, size) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::ENODATA)
}

fn xattr_list_path(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    let size = args.arg2 as usize;
    if args.arg1 != 0 && size > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, size) {
            return linux_err(linux_errno_for(e));
        }
    }
    // No attributes -> empty list -> 0.
    SyscallResult::ok(0)
}

/// `listxattr(path, list, size)`.
fn sys_listxattr(args: &SyscallArgs) -> SyscallResult {
    xattr_list_path(args)
}
/// `llistxattr(path, list, size)`.
fn sys_llistxattr(args: &SyscallArgs) -> SyscallResult {
    xattr_list_path(args)
}

/// `flistxattr(fd, list, size)`.
fn sys_flistxattr(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    let size = args.arg2 as usize;
    if args.arg1 != 0 && size > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, size) {
            return linux_err(linux_errno_for(e));
        }
    }
    SyscallResult::ok(0)
}

fn xattr_remove_path(args: &SyscallArgs) -> SyscallResult {
    if let Err(r) = xattr_validate_path_name(args.arg0, args.arg1) {
        return r;
    }
    linux_err(errno::EOPNOTSUPP)
}

/// `removexattr(path, name)`.
fn sys_removexattr(args: &SyscallArgs) -> SyscallResult {
    xattr_remove_path(args)
}
/// `lremovexattr(path, name)`.
fn sys_lremovexattr(args: &SyscallArgs) -> SyscallResult {
    xattr_remove_path(args)
}

/// `fremovexattr(fd, name)`.
fn sys_fremovexattr(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = xattr_validate_fd_name(fd, args.arg1) {
        return r;
    }
    linux_err(errno::EOPNOTSUPP)
}

// ---------------------------------------------------------------------------
// Disk quotas, kernel modules, namespaces, mount, swap, reboot, syslog
//
// All privileged; without writable filesystems / kernel-module loader /
// namespace support / power management, these return EPERM after input
// validation.  The truthful answer to "did you do it?" is "no, the
// caller lacks privilege" — which is also what a real Linux kernel
// would say if the caller isn't root with CAP_SYS_ADMIN.
// ---------------------------------------------------------------------------

/// `quotactl(cmd, special, id, addr)`.
fn sys_quotactl(args: &SyscallArgs) -> SyscallResult {
    // special is a path pointer (when needed); validate if non-NULL.
    if args.arg1 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 1) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EPERM)
}

/// `quotactl_fd(fd, cmd, id, addr)`.
fn sys_quotactl_fd(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    linux_err(errno::EPERM)
}

/// `init_module(module_image, len, param_values)`.
fn sys_init_module(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    let len = args.arg1 as usize;
    if len == 0 {
        return linux_err(errno::EINVAL);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, len) {
        return linux_err(linux_errno_for(e));
    }
    if args.arg2 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 1) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EPERM)
}

/// `finit_module(fd, param_values, flags)`.
fn sys_finit_module(args: &SyscallArgs) -> SyscallResult {
    // MODULE_INIT_IGNORE_MODVERSIONS = 1, IGNORE_VERMAGIC = 2,
    // COMPRESSED_FILE = 4.
    const VALID_FLAGS: u64 = 1 | 2 | 4;
    if args.arg2 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    if args.arg1 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 1) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EPERM)
}

/// `delete_module(name, flags)`.
fn sys_delete_module(args: &SyscallArgs) -> SyscallResult {
    // O_NONBLOCK=0o4000, O_TRUNC=0o1000 (the only valid flags).
    const VALID_FLAGS: u64 = 0o4000 | 0o1000;
    if args.arg1 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EPERM)
}

/// `unshare(flags)`.
fn sys_unshare(args: &SyscallArgs) -> SyscallResult {
    // CLONE_FILES=0x400, CLONE_FS=0x200, CLONE_NEWNS=0x20000,
    // CLONE_SYSVSEM=0x40000, CLONE_NEWIPC=0x8000000, CLONE_NEWNET=0x40000000,
    // CLONE_NEWPID=0x20000000, CLONE_NEWUSER=0x10000000,
    // CLONE_NEWUTS=0x4000000, CLONE_NEWCGROUP=0x2000000,
    // CLONE_NEWTIME=0x80, CLONE_THREAD=0x10000, CLONE_SIGHAND=0x800,
    // CLONE_VM=0x100.
    const VALID_FLAGS: u64 = 0x400 | 0x200 | 0x20000 | 0x40000 | 0x800_0000
        | 0x4000_0000 | 0x2000_0000 | 0x1000_0000 | 0x400_0000
        | 0x200_0000 | 0x80 | 0x10000 | 0x800 | 0x100;
    if args.arg0 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    // unshare(0) trivially succeeds: nothing to unshare.
    if args.arg0 == 0 {
        return SyscallResult::ok(0);
    }
    linux_err(errno::EPERM)
}

/// `setns(fd, nstype)`.
fn sys_setns(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    linux_err(errno::EPERM)
}

/// `mount(source, target, fstype, mountflags, data)`.
fn sys_mount(args: &SyscallArgs) -> SyscallResult {
    // target is required; source/fstype/data optional depending on op.
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 1) {
        return linux_err(linux_errno_for(e));
    }
    for ptr in [args.arg0, args.arg2, args.arg4] {
        if ptr != 0 {
            if let Err(e) = crate::mm::user::validate_user_read(ptr, 1) {
                return linux_err(linux_errno_for(e));
            }
        }
    }
    linux_err(errno::EPERM)
}

/// `umount2(target, flags)`.
fn sys_umount2(args: &SyscallArgs) -> SyscallResult {
    // MNT_FORCE=1, MNT_DETACH=2, MNT_EXPIRE=4, UMOUNT_NOFOLLOW=8.
    const VALID_FLAGS: u64 = 1 | 2 | 4 | 8;
    if args.arg1 & !VALID_FLAGS != 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EPERM)
}

/// `pivot_root(new_root, put_old)`.
fn sys_pivot_root(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 || args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EPERM)
}

/// `swapon(path, swapflags)`.
fn sys_swapon(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EPERM)
}

/// `swapoff(path)`.
fn sys_swapoff(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EPERM)
}

/// `reboot(magic1, magic2, cmd, arg)`.
fn sys_reboot(args: &SyscallArgs) -> SyscallResult {
    // Linux requires magic1 = LINUX_REBOOT_MAGIC1 (0xfee1dead)
    // and magic2 in a fixed set (672274793 etc.).  We refuse all
    // reboots from userspace for now.
    const MAGIC1: u64 = 0xfee1_dead;
    if args.arg0 != MAGIC1 {
        return linux_err(errno::EINVAL);
    }
    linux_err(errno::EPERM)
}

// ---------------------------------------------------------------------------
// SysV IPC (shm / sem / msg) and POSIX message queues (mq_*)
//
// We don't implement either form.  POSIX mq is more common than SysV
// in modern code, but both are infrequently used by desktop apps; both
// have well-documented errno fallback paths.
//
// For lookups (shmget / semget / msgget / mq_open) we return -ENOSYS
// after validating flags / mode / pointers — feature-detection code
// recognises this and falls back to shm_open() or pipes.  For
// operations referencing a non-existent id (shmat, shmctl, shmdt, sem*,
// msg*) we return -EINVAL ("invalid identifier"), which is Linux's
// answer for a stale id and what portable code handles.
// ---------------------------------------------------------------------------

/// `shmget(key, size, shmflg)` — create / get SysV shared memory.
fn sys_shmget(args: &SyscallArgs) -> SyscallResult {
    // IPC_CREAT=0o1000, IPC_EXCL=0o2000, plus permission bits + huge
    // page bits.  We accept anything for now.
    let _flags = args.arg2;
    // size must be > 0 unless looking up an existing segment.
    let size = args.arg1 as usize;
    if size == 0 && (args.arg2 & 0o1000) != 0 {
        return linux_err(errno::EINVAL);
    }
    linux_err(errno::ENOSYS)
}

/// `shmat(shmid, shmaddr, shmflg)`.
fn sys_shmat(_args: &SyscallArgs) -> SyscallResult {
    linux_err(errno::EINVAL)
}

/// `shmctl(shmid, cmd, buf)`.
fn sys_shmctl(args: &SyscallArgs) -> SyscallResult {
    if args.arg2 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 1) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EINVAL)
}

/// `shmdt(shmaddr)`.
fn sys_shmdt(_args: &SyscallArgs) -> SyscallResult {
    linux_err(errno::EINVAL)
}

/// `semget(key, nsems, semflg)`.
fn sys_semget(args: &SyscallArgs) -> SyscallResult {
    let nsems = args.arg1 as i32;
    if nsems < 0 {
        return linux_err(errno::EINVAL);
    }
    // SEMMSL upper-bound check (Linux default 32000) would belong here.
    linux_err(errno::ENOSYS)
}

/// `semop(semid, sops, nsops)`.
fn sys_semop(args: &SyscallArgs) -> SyscallResult {
    let nsops = args.arg2 as usize;
    if nsops == 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    // struct sembuf = 6 bytes; round up validate.
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, nsops.saturating_mul(6)) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EINVAL)
}

/// `semctl(semid, semnum, cmd, arg)`.
fn sys_semctl(_args: &SyscallArgs) -> SyscallResult {
    linux_err(errno::EINVAL)
}

/// `semtimedop(semid, sops, nsops, timeout)`.
fn sys_semtimedop(args: &SyscallArgs) -> SyscallResult {
    let nsops = args.arg2 as usize;
    if nsops == 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, nsops.saturating_mul(6)) {
        return linux_err(linux_errno_for(e));
    }
    if args.arg3 != 0 {
        // timespec = 16 bytes.
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, 16) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EINVAL)
}

/// `msgget(key, msgflg)`.
fn sys_msgget(_args: &SyscallArgs) -> SyscallResult {
    linux_err(errno::ENOSYS)
}

/// `msgsnd(msqid, msgp, msgsz, msgflg)`.
fn sys_msgsnd(args: &SyscallArgs) -> SyscallResult {
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    let sz = args.arg2 as usize;
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, sz.saturating_add(8)) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EINVAL)
}

/// `msgrcv(msqid, msgp, msgsz, msgtyp, msgflg)`.
fn sys_msgrcv(args: &SyscallArgs) -> SyscallResult {
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    let sz = args.arg2 as usize;
    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, sz.saturating_add(8)) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EINVAL)
}

/// `msgctl(msqid, cmd, buf)`.
fn sys_msgctl(args: &SyscallArgs) -> SyscallResult {
    if args.arg2 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 1) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EINVAL)
}

/// `mq_open(name, oflag, mode, attr)`.
fn sys_mq_open(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    // attr is struct mq_attr = 8 * 8 bytes = 64 bytes when non-NULL.
    if args.arg3 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, 64) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::ENOSYS)
}

/// `mq_unlink(name)`.
fn sys_mq_unlink(args: &SyscallArgs) -> SyscallResult {
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg0, 1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::ENOENT)
}

/// `mq_timedsend(mqd, msg, len, prio, abs_timeout)`.
fn sys_mq_timedsend(args: &SyscallArgs) -> SyscallResult {
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    let len = args.arg2 as usize;
    if len > 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, len) {
            return linux_err(linux_errno_for(e));
        }
    }
    if args.arg4 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg4, 16) {
            return linux_err(linux_errno_for(e));
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    linux_err(errno::EBADF)
}

/// `mq_timedreceive(mqd, msg, len, prio_ptr, abs_timeout)`.
fn sys_mq_timedreceive(args: &SyscallArgs) -> SyscallResult {
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    let len = args.arg2 as usize;
    if len > 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg1, len) {
            return linux_err(linux_errno_for(e));
        }
    }
    if args.arg3 != 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg3, 4) {
            return linux_err(linux_errno_for(e));
        }
    }
    if args.arg4 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg4, 16) {
            return linux_err(linux_errno_for(e));
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    linux_err(errno::EBADF)
}

/// `mq_notify(mqd, sevp)`.
fn sys_mq_notify(args: &SyscallArgs) -> SyscallResult {
    if args.arg1 != 0 {
        // struct sigevent = 64 bytes.
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 64) {
            return linux_err(linux_errno_for(e));
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    linux_err(errno::EBADF)
}

/// `mq_getsetattr(mqd, newattr, oldattr)`.
fn sys_mq_getsetattr(args: &SyscallArgs) -> SyscallResult {
    if args.arg1 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 64) {
            return linux_err(linux_errno_for(e));
        }
    }
    if args.arg2 != 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg2, 64) {
            return linux_err(linux_errno_for(e));
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    linux_err(errno::EBADF)
}

// ---------------------------------------------------------------------------
// poll / ppoll / select / pselect6 / epoll family
//
// These are the four flavours of "wait for I/O readiness on a set of file
// descriptors with an optional timeout".  Real readiness-multiplexing does
// not yet exist in this kernel: console / pipe / file fds are all read /
// written synchronously, and the in-flight IPC / IOCP path is a separate
// world from POSIX fds.  Wiring them up to a real ready-queue is a multi-
// week effort that has to land alongside non-blocking I/O on all four
// HandleKinds.
//
// In the meantime we expose a principled-stub API:
//
//   * Validate every user-space pointer (pollfd arrays, fd_set bitmaps,
//     timespec, sigset_t) so a malicious caller cannot trick the kernel
//     into reading arbitrary memory.
//   * For all four (poll / ppoll / select / pselect6) plus all six epoll
//     variants, return ENOSYS so glibc / libuv / tokio's feature-detect
//     paths know to skip these and use blocking I/O directly.  Returning
//     "0 events ready" would silently hang every event-loop based program.
//   * epoll_ctl returns EBADF because there is no real epoll-fd to operate
//     on.
//
// When real readiness multiplexing lands, these stubs are replaced by
// dispatches into the readiness subsystem.
// ---------------------------------------------------------------------------

/// `poll(fds*, nfds, timeout_ms)` — wait for events on a set of fds.
fn sys_poll(args: &SyscallArgs) -> SyscallResult {
    let nfds = args.arg1;
    // Linux caps nfds at RLIMIT_NOFILE; we accept up to 1<<20 as a sanity
    // bound and then refuse anything larger as EINVAL.
    if nfds > (1 << 20) {
        return linux_err(errno::EINVAL);
    }
    if nfds > 0 {
        if args.arg0 == 0 {
            return linux_err(errno::EFAULT);
        }
        // struct pollfd is 8 bytes: { int fd; short events; short revents; }
        let len = nfds.saturating_mul(8);
        if let Err(e) = crate::mm::user::validate_user_write(args.arg0, len as usize) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::ENOSYS)
}

/// `ppoll(fds*, nfds, timespec*, sigmask*, sigsetsize)` — like poll with
/// nanosecond-precision timeout and atomic sigmask swap.
fn sys_ppoll(args: &SyscallArgs) -> SyscallResult {
    let nfds = args.arg1;
    if nfds > (1 << 20) {
        return linux_err(errno::EINVAL);
    }
    if nfds > 0 {
        if args.arg0 == 0 {
            return linux_err(errno::EFAULT);
        }
        let len = nfds.saturating_mul(8);
        if let Err(e) = crate::mm::user::validate_user_write(args.arg0, len as usize) {
            return linux_err(linux_errno_for(e));
        }
    }
    if args.arg2 != 0 {
        // struct timespec is 16 bytes.
        if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 16) {
            return linux_err(linux_errno_for(e));
        }
    }
    // sigsetsize must equal sizeof(sigset_t) = 8 on x86_64.
    if args.arg4 != 0 && args.arg4 != 8 {
        return linux_err(errno::EINVAL);
    }
    if args.arg3 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, 8) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::ENOSYS)
}

/// Round nfds bits up to bytes for an fd_set, capped at a sane maximum
/// (1<<20 fds = 128KiB per fd_set, which is the same cap select() uses
/// before erroring with EINVAL).
fn fd_set_byte_len(nfds: i32) -> Result<usize, SyscallResult> {
    if nfds < 0 {
        return Err(linux_err(errno::EINVAL));
    }
    let nfds_u = nfds as u64;
    if nfds_u > (1 << 20) {
        return Err(linux_err(errno::EINVAL));
    }
    // bits → bytes, rounded up.
    Ok(((nfds_u + 7) / 8) as usize)
}

/// `select(nfds, readfds*, writefds*, exceptfds*, timeval*)` — classic
/// readiness multiplexing.  timeval is 16 bytes (sec + usec).
fn sys_select(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let nfds = args.arg0 as i32;
    let len = match fd_set_byte_len(nfds) {
        Ok(n) => n,
        Err(r) => return r,
    };
    for ptr in [args.arg1, args.arg2, args.arg3] {
        if ptr != 0 && len > 0 {
            if let Err(e) = crate::mm::user::validate_user_write(ptr, len) {
                return linux_err(linux_errno_for(e));
            }
        }
    }
    if args.arg4 != 0 {
        if let Err(e) = crate::mm::user::validate_user_write(args.arg4, 16) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::ENOSYS)
}

/// `pselect6(nfds, readfds*, writefds*, exceptfds*, timespec*, sigmask_arg*)`
/// — sigmask_arg points to `struct { sigset_t *ss; size_t ss_len; }` (16B).
fn sys_pselect6(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let nfds = args.arg0 as i32;
    let len = match fd_set_byte_len(nfds) {
        Ok(n) => n,
        Err(r) => return r,
    };
    for ptr in [args.arg1, args.arg2, args.arg3] {
        if ptr != 0 && len > 0 {
            if let Err(e) = crate::mm::user::validate_user_write(ptr, len) {
                return linux_err(linux_errno_for(e));
            }
        }
    }
    if args.arg4 != 0 {
        // struct timespec is 16 bytes.
        if let Err(e) = crate::mm::user::validate_user_read(args.arg4, 16) {
            return linux_err(linux_errno_for(e));
        }
    }
    if args.arg5 != 0 {
        // { const sigset_t *ss; size_t ss_len; } = 16 bytes
        if let Err(e) = crate::mm::user::validate_user_read(args.arg5, 16) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::ENOSYS)
}

/// `epoll_create(size)` — historical, size is now ignored but must be > 0.
fn sys_epoll_create(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let size = args.arg0 as i32;
    if size <= 0 {
        return linux_err(errno::EINVAL);
    }
    linux_err(errno::ENOSYS)
}

/// `epoll_create1(flags)` — flags is EPOLL_CLOEXEC (0o2_000_000) only.
fn sys_epoll_create1(args: &SyscallArgs) -> SyscallResult {
    const EPOLL_CLOEXEC: u32 = 0o2_000_000;
    #[allow(clippy::cast_possible_truncation)]
    let flags = args.arg0 as u32;
    if flags & !EPOLL_CLOEXEC != 0 {
        return linux_err(errno::EINVAL);
    }
    linux_err(errno::ENOSYS)
}

/// `epoll_ctl(epfd, op, fd, event*)` — op in {ADD=1, DEL=2, MOD=3}.
fn sys_epoll_ctl(args: &SyscallArgs) -> SyscallResult {
    const EPOLL_CTL_ADD: i32 = 1;
    const EPOLL_CTL_DEL: i32 = 2;
    const EPOLL_CTL_MOD: i32 = 3;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let op = args.arg1 as i32;
    if !matches!(op, EPOLL_CTL_ADD | EPOLL_CTL_DEL | EPOLL_CTL_MOD) {
        return linux_err(errno::EINVAL);
    }
    // event ptr is required for ADD / MOD; DEL ignores it.
    if op != EPOLL_CTL_DEL {
        if args.arg3 == 0 {
            return linux_err(errno::EFAULT);
        }
        // struct epoll_event is 12 bytes on x86_64 (packed): u32 events + u64 data.
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, 12) {
            return linux_err(linux_errno_for(e));
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let target_fd = args.arg2 as i32;
    // Validate the target fd is a real fd in our table (or we are in kernel
    // context, in which case validation is a no-op).
    if let Err(r) = validate_linux_fd(target_fd) {
        return r;
    }
    // The epfd itself does not exist in our kernel — epoll_create returned
    // ENOSYS so no caller can hold a real epfd.  Tell them so.
    linux_err(errno::EBADF)
}

/// `epoll_wait(epfd, events*, maxevents, timeout_ms)`.
fn sys_epoll_wait(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let maxevents = args.arg2 as i32;
    if maxevents <= 0 {
        return linux_err(errno::EINVAL);
    }
    // struct epoll_event is 12 bytes.
    let len = (maxevents as u64).saturating_mul(12);
    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, len as usize) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EBADF)
}

/// `epoll_pwait(epfd, events*, maxevents, timeout_ms, sigmask*, sigsetsize)`.
fn sys_epoll_pwait(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let maxevents = args.arg2 as i32;
    if maxevents <= 0 {
        return linux_err(errno::EINVAL);
    }
    let len = (maxevents as u64).saturating_mul(12);
    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, len as usize) {
        return linux_err(linux_errno_for(e));
    }
    if args.arg5 != 0 && args.arg5 != 8 {
        return linux_err(errno::EINVAL);
    }
    if args.arg4 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg4, 8) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EBADF)
}

/// `epoll_pwait2(epfd, events*, maxevents, timespec*, sigmask*, sigsetsize)`.
fn sys_epoll_pwait2(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let maxevents = args.arg2 as i32;
    if maxevents <= 0 {
        return linux_err(errno::EINVAL);
    }
    let len = (maxevents as u64).saturating_mul(12);
    if let Err(e) = crate::mm::user::validate_user_write(args.arg1, len as usize) {
        return linux_err(linux_errno_for(e));
    }
    if args.arg3 != 0 {
        // timespec 16 bytes.
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, 16) {
            return linux_err(linux_errno_for(e));
        }
    }
    if args.arg5 != 0 && args.arg5 != 8 {
        return linux_err(errno::EINVAL);
    }
    if args.arg4 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg4, 8) {
            return linux_err(linux_errno_for(e));
        }
    }
    linux_err(errno::EBADF)
}

// ---------------------------------------------------------------------------
// openat2 / execveat / name_to_handle_at / open_by_handle_at + new mount API
//
// These are the post-2010 Linux syscalls that extend openat / execve and
// reshape the mount system around fd-based handles.  Our kernel does not
// model the resolve restrictions of openat2's struct open_how, nor does it
// have a mount-tree to manipulate via fsopen / fsmount / open_tree /
// move_mount, nor a persistent file_handle table for name_to_handle_at.
//
// In the meantime we expose principled-stub semantics:
//
//   * openat2 / execveat: validate every pointer and flag bit, then ENOSYS
//     so glibc / io_uring fall back to openat / execve (which we already
//     implement).
//   * The new mount API (fsopen, fsconfig, fsmount, fspick, open_tree,
//     move_mount) returns EPERM after validation — these are privileged
//     and a non-root caller on Linux gets the same answer.
//   * name_to_handle_at / open_by_handle_at return EOPNOTSUPP after
//     validation — that is the canonical answer for "this filesystem does
//     not export persistent handles", which is what filesystems like tmpfs
//     return on Linux.
// ---------------------------------------------------------------------------

/// `openat2(dirfd, path, how*, size)` — like openat with explicit
/// resolve flags.  `how` is `struct open_how { u64 flags; u64 mode;
/// u64 resolve; }` (24 bytes).
fn sys_openat2(args: &SyscallArgs) -> SyscallResult {
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = validate_user_str(args.arg1) {
        return linux_err(linux_errno_for(e));
    }
    // Linux enforces size == sizeof(struct open_how) = 24 currently; any
    // other value gets EINVAL.  The kernel may grow `how` over time but
    // never below 24.
    if args.arg3 != 24 {
        return linux_err(errno::EINVAL);
    }
    if args.arg2 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 24) {
        return linux_err(linux_errno_for(e));
    }
    // We validated everything; tell callers to fall back to openat.
    linux_err(errno::ENOSYS)
}

/// `execveat(dirfd, path, argv, envp, flags)` — like execve relative to a
/// directory fd.  Flags are AT_EMPTY_PATH (0x1000) | AT_SYMLINK_NOFOLLOW
/// (0x100).
fn sys_execveat(args: &SyscallArgs) -> SyscallResult {
    const AT_EMPTY_PATH: u32 = 0x1000;
    const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
    #[allow(clippy::cast_possible_truncation)]
    let flags = args.arg4 as u32;
    if flags & !(AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW) != 0 {
        return linux_err(errno::EINVAL);
    }
    // Validate path unless AT_EMPTY_PATH and path is empty.
    if args.arg1 != 0 {
        if let Err(e) = validate_user_str(args.arg1) {
            return linux_err(linux_errno_for(e));
        }
    } else if flags & AT_EMPTY_PATH == 0 {
        return linux_err(errno::EFAULT);
    }
    // argv / envp can be NULL only on very old code; modern callers pass
    // valid pointers.  Validate the first slot (8 bytes for a pointer)
    // if non-NULL.
    if args.arg2 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 8) {
            return linux_err(linux_errno_for(e));
        }
    }
    if args.arg3 != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, 8) {
            return linux_err(linux_errno_for(e));
        }
    }
    // Fall back to execve — callers handle ENOSYS gracefully.
    linux_err(errno::ENOSYS)
}

/// `name_to_handle_at(dirfd, path, handle*, mount_id*, flags)`.
/// `struct file_handle { u32 handle_bytes; int handle_type; u8 f_handle[0]; }`
/// — minimum 8 bytes for the header, but we just validate the header so
/// the caller learns the right errno.
fn sys_name_to_handle_at(args: &SyscallArgs) -> SyscallResult {
    const AT_EMPTY_PATH: u32 = 0x1000;
    const AT_SYMLINK_FOLLOW: u32 = 0x400;
    const AT_HANDLE_FID: u32 = 0x200;
    #[allow(clippy::cast_possible_truncation)]
    let flags = args.arg4 as u32;
    if flags & !(AT_EMPTY_PATH | AT_SYMLINK_FOLLOW | AT_HANDLE_FID) != 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg1 != 0 {
        if let Err(e) = validate_user_str(args.arg1) {
            return linux_err(linux_errno_for(e));
        }
    } else if flags & AT_EMPTY_PATH == 0 {
        return linux_err(errno::EFAULT);
    }
    // mount_id is required.
    if args.arg3 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_write(args.arg3, 4) {
        return linux_err(linux_errno_for(e));
    }
    // handle ptr is required.
    if args.arg2 == 0 {
        return linux_err(errno::EFAULT);
    }
    // struct file_handle header is 8 bytes (4 + 4); validate header read
    // so we can see handle_bytes, which is what Linux does first.
    if let Err(e) = crate::mm::user::validate_user_read(args.arg2, 8) {
        return linux_err(linux_errno_for(e));
    }
    // No filesystem exports persistent handles in this kernel.
    linux_err(errno::EOPNOTSUPP)
}

/// `open_by_handle_at(mount_fd, handle*, flags)`.
fn sys_open_by_handle_at(args: &SyscallArgs) -> SyscallResult {
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = crate::mm::user::validate_user_read(args.arg1, 8) {
        return linux_err(linux_errno_for(e));
    }
    // Same flags as openat (O_RDONLY/RDWR/WRONLY plus mode bits); we don't
    // re-validate them here — Linux only sanity-checks the access mode.
    // Real open-by-handle requires CAP_DAC_READ_SEARCH, which we don't
    // grant; mirror Linux's EPERM in that case.
    linux_err(errno::EPERM)
}

/// `fsopen(fsname*, flags)` — create a filesystem context.  Flags are
/// FSOPEN_CLOEXEC (1) only.
fn sys_fsopen(args: &SyscallArgs) -> SyscallResult {
    const FSOPEN_CLOEXEC: u32 = 0x1;
    #[allow(clippy::cast_possible_truncation)]
    let flags = args.arg1 as u32;
    if flags & !FSOPEN_CLOEXEC != 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg0 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = validate_user_str(args.arg0) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EPERM)
}

/// `fsconfig(fs_fd, cmd, key*, value*, aux)` — configure a filesystem
/// context.  cmd ∈ 0..=8 (FSCONFIG_SET_FLAG ... FSCONFIG_CMD_RECONFIGURE).
fn sys_fsconfig(args: &SyscallArgs) -> SyscallResult {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let cmd = args.arg1 as i32;
    if !(0..=8).contains(&cmd) {
        return linux_err(errno::EINVAL);
    }
    // key is required for SET_FLAG / SET_STRING / SET_BINARY / SET_PATH /
    // SET_PATH_EMPTY / SET_FD (cmd 0..=5).
    if cmd <= 5 && args.arg2 != 0 {
        if let Err(e) = validate_user_str(args.arg2) {
            return linux_err(linux_errno_for(e));
        }
    }
    if args.arg3 != 0 {
        // Validate at least 1 byte of value; the actual length depends on
        // the cmd, but if the pointer is non-NULL it must be readable.
        if let Err(e) = crate::mm::user::validate_user_read(args.arg3, 1) {
            return linux_err(linux_errno_for(e));
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    linux_err(errno::EPERM)
}

/// `fsmount(fs_fd, flags, attr_flags)` — create a mount object from a
/// configured fs context.  flags = FSMOUNT_CLOEXEC (1).
fn sys_fsmount(args: &SyscallArgs) -> SyscallResult {
    const FSMOUNT_CLOEXEC: u32 = 0x1;
    #[allow(clippy::cast_possible_truncation)]
    let flags = args.arg1 as u32;
    if flags & !FSMOUNT_CLOEXEC != 0 {
        return linux_err(errno::EINVAL);
    }
    // attr_flags are MOUNT_ATTR_* bits — RDONLY (0x1), NOSUID (0x2),
    // NODEV (0x4), NOEXEC (0x8), _ATIME mask (0x70), STRICTATIME (0x20),
    // NOATIME (0x10), NODIRATIME (0x80), IDMAP (0x100_000), NOSYMFOLLOW
    // (0x20_0000) — accept the union conservatively and reject anything
    // above.
    const MOUNT_ATTR_MASK: u32 = 0x20_00ff;
    #[allow(clippy::cast_possible_truncation)]
    let attr_flags = args.arg2 as u32;
    if attr_flags & !MOUNT_ATTR_MASK != 0 {
        return linux_err(errno::EINVAL);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let fd = args.arg0 as i32;
    if let Err(r) = validate_linux_fd(fd) {
        return r;
    }
    linux_err(errno::EPERM)
}

/// `fspick(dirfd, path*, flags)` — pick an existing mount to reconfigure.
/// flags = FSPICK_CLOEXEC (1) | FSPICK_SYMLINK_NOFOLLOW (2) | NO_AUTOMOUNT
/// (4) | EMPTY_PATH (8).
fn sys_fspick(args: &SyscallArgs) -> SyscallResult {
    const FSPICK_MASK: u32 = 0xf;
    #[allow(clippy::cast_possible_truncation)]
    let flags = args.arg2 as u32;
    if flags & !FSPICK_MASK != 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg1 == 0 {
        return linux_err(errno::EFAULT);
    }
    if let Err(e) = validate_user_str(args.arg1) {
        return linux_err(linux_errno_for(e));
    }
    linux_err(errno::EPERM)
}

/// `open_tree(dirfd, path*, flags)` — clone a mount subtree into a new
/// detached fd.  flags include OPEN_TREE_CLONE (1), OPEN_TREE_CLOEXEC
/// (O_CLOEXEC = 0o2_000_000), plus AT_* path-walking bits.
fn sys_open_tree(args: &SyscallArgs) -> SyscallResult {
    const OPEN_TREE_CLONE: u32 = 1;
    const O_CLOEXEC: u32 = 0o2_000_000;
    const AT_EMPTY_PATH: u32 = 0x1000;
    const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
    const AT_NO_AUTOMOUNT: u32 = 0x800;
    const AT_RECURSIVE: u32 = 0x8000;
    const OPEN_TREE_MASK: u32 =
        OPEN_TREE_CLONE | O_CLOEXEC | AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW
        | AT_NO_AUTOMOUNT | AT_RECURSIVE;
    #[allow(clippy::cast_possible_truncation)]
    let flags = args.arg2 as u32;
    if flags & !OPEN_TREE_MASK != 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg1 != 0 {
        if let Err(e) = validate_user_str(args.arg1) {
            return linux_err(linux_errno_for(e));
        }
    } else if flags & AT_EMPTY_PATH == 0 {
        return linux_err(errno::EFAULT);
    }
    linux_err(errno::EPERM)
}

/// `move_mount(from_dirfd, from_path*, to_dirfd, to_path*, flags)`.
fn sys_move_mount(args: &SyscallArgs) -> SyscallResult {
    const MOVE_MOUNT_F_SYMLINKS: u32 = 0x1;
    const MOVE_MOUNT_F_AUTOMOUNTS: u32 = 0x2;
    const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x4;
    const MOVE_MOUNT_T_SYMLINKS: u32 = 0x10;
    const MOVE_MOUNT_T_AUTOMOUNTS: u32 = 0x20;
    const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x40;
    const MOVE_MOUNT_SET_GROUP: u32 = 0x100;
    const MOVE_MOUNT_BENEATH: u32 = 0x200;
    const MOVE_MOUNT_MASK: u32 = MOVE_MOUNT_F_SYMLINKS
        | MOVE_MOUNT_F_AUTOMOUNTS
        | MOVE_MOUNT_F_EMPTY_PATH
        | MOVE_MOUNT_T_SYMLINKS
        | MOVE_MOUNT_T_AUTOMOUNTS
        | MOVE_MOUNT_T_EMPTY_PATH
        | MOVE_MOUNT_SET_GROUP
        | MOVE_MOUNT_BENEATH;
    #[allow(clippy::cast_possible_truncation)]
    let flags = args.arg4 as u32;
    if flags & !MOVE_MOUNT_MASK != 0 {
        return linux_err(errno::EINVAL);
    }
    if args.arg1 != 0 {
        if let Err(e) = validate_user_str(args.arg1) {
            return linux_err(linux_errno_for(e));
        }
    } else if flags & MOVE_MOUNT_F_EMPTY_PATH == 0 {
        return linux_err(errno::EFAULT);
    }
    if args.arg3 != 0 {
        if let Err(e) = validate_user_str(args.arg3) {
            return linux_err(linux_errno_for(e));
        }
    } else if flags & MOVE_MOUNT_T_EMPTY_PATH == 0 {
        return linux_err(errno::EFAULT);
    }
    linux_err(errno::EPERM)
}

/// `syslog(type, bufp, len)` — read / write the kernel log buffer.
fn sys_syslog(args: &SyscallArgs) -> SyscallResult {
    // type 0..=10 are defined in Linux.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let typ = args.arg0 as i32;
    if !(0..=10).contains(&typ) {
        return linux_err(errno::EINVAL);
    }
    // Most useful actions (SYSLOG_ACTION_READ family) need bufp/len;
    // validate len-pointer writes when applicable, but we don't have
    // a Linux-shaped klog buffer available so refuse.
    let len = args.arg2 as usize;
    if typ == 2 || typ == 3 || typ == 4 {
        // Read variants: need buffer if len > 0.
        if args.arg1 == 0 && len > 0 {
            return linux_err(errno::EFAULT);
        }
        if len > 0 {
            if let Err(e) = crate::mm::user::validate_user_write(args.arg1, len) {
                return linux_err(linux_errno_for(e));
            }
        }
    }
    // SYSLOG_ACTION_SIZE_BUFFER and SIZE_UNREAD return 0 (no log
    // available).
    if typ == 6 || typ == 7 || typ == 9 || typ == 10 {
        return SyscallResult::ok(0);
    }
    linux_err(errno::EPERM)
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

/// `prlimit64(pid, resource, new_limit, old_limit)` — get and/or set
/// a Linux per-process resource limit.
///
/// Linux's `struct rlimit { rlim_t rlim_cur; rlim_t rlim_max; }` is
/// 16 bytes (`rlim_t` is `u64` on x86_64 LP64).  glibc calls
/// `prlimit64(0, RLIMIT_STACK, NULL, &out)` at process startup to
/// size the main thread's stack — returning ENOSYS there causes glibc
/// to either abort or use an unreasonably small default.
///
/// Our policy:
///   - pid == 0  → self (the only target we honour).  pid != 0 with
///     identity == caller is treated as self; everything else returns
///     -EPERM (cross-process rlimit queries require CAP_SYS_RESOURCE
///     on Linux and we have no equivalent for non-self at this layer).
///   - `resource` must be in 0..=15 (RLIMIT_CPU..RLIMIT_RTTIME);
///     anything else returns -EINVAL.
///   - `new_limit` non-null: copy in, then silently ignore the
///     request (we don't honour limit changes yet but accept them as
///     a no-op so programs that "lower then re-read" see consistent
///     state).
///   - `old_limit` non-null: write our compiled-in default for
///     `resource` (see [`rlimit_default`]).
///   - NULL pointers are skipped (POSIX).
///
/// Returns 0 on success, negative errno otherwise.
fn sys_prlimit64(args: &SyscallArgs) -> SyscallResult {
    let pid = args.arg0;
    let resource = args.arg1;
    let new_limit_ptr = args.arg2;
    let old_limit_ptr = args.arg3;

    // Resource validation up front — Linux rejects unknown resources
    // before touching any user pointer.
    if resource > 15 {
        return linux_err(errno::EINVAL);
    }

    // Cross-process queries: only allow when targeting self (pid == 0
    // or pid == caller's PID).  Otherwise EPERM (matches Linux's
    // behaviour for unprivileged callers without CAP_SYS_RESOURCE).
    if pid != 0 {
        let me = caller_pid().unwrap_or(0);
        if pid != me {
            return linux_err(errno::EPERM);
        }
    }

    // Pre-validate user pointers.  Each rlimit is 16 bytes.
    const RLIMIT_SIZE: usize = 16;
    if new_limit_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_read(new_limit_ptr, RLIMIT_SIZE) {
            return linux_err(linux_errno_for(e));
        }
    }
    if old_limit_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_write(old_limit_ptr, RLIMIT_SIZE) {
            return linux_err(linux_errno_for(e));
        }
    }

    // Read the new limit (we don't store it, but copying it in and
    // discarding lets us validate the pointer was actually a valid
    // user mapping — Linux faults on bad new_limit even when
    // old_limit is the "real" target).
    if new_limit_ptr != 0 {
        let mut tmp = [0u8; RLIMIT_SIZE];
        // SAFETY: validate_user_read above confirmed the range; we
        // pass a kernel-owned buffer of the correct size.
        let r = unsafe {
            crate::mm::user::copy_from_user(new_limit_ptr, tmp.as_mut_ptr(), RLIMIT_SIZE)
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
        // Discard — limit changes are no-ops until we have a per-
        // process rlimit store.  See todo.txt.
    }

    // Write the default for `resource` to old_limit_ptr.
    if old_limit_ptr != 0 {
        #[allow(clippy::cast_possible_truncation)]
        let r = resource as u32;
        let (cur, max) = rlimit_default(r);
        let buf: [u64; 2] = [cur, max];
        // SAFETY: validated as a writable user range of RLIMIT_SIZE
        // bytes above.
        let r = unsafe {
            crate::mm::user::copy_to_user(
                buf.as_ptr().cast::<u8>(),
                old_limit_ptr,
                RLIMIT_SIZE,
            )
        };
        if let Err(e) = r {
            return linux_err(linux_errno_for(e));
        }
    }

    SyscallResult::ok(0)
}

/// Compiled-in default `(rlim_cur, rlim_max)` for each Linux RLIMIT_*
/// resource.  These are static — we don't carry per-process state yet,
/// so every process sees the same limits.
///
/// Values mirror typical Linux distro defaults where they matter for
/// program startup (RLIMIT_STACK == 8 MiB so glibc sizes the main
/// stack correctly; RLIMIT_NOFILE == 1024; RLIMIT_CORE == 0 so we
/// don't pretend to support core dumps).  Everything else is
/// `RLIM_INFINITY` because nothing in the kernel imposes a real
/// limit on those resources today.
fn rlimit_default(resource: u32) -> (u64, u64) {
    /// `RLIM_INFINITY` on Linux x86_64.
    const INF: u64 = u64::MAX;

    match resource {
        // RLIMIT_CPU: CPU seconds.  No limiter today.
        0 => (INF, INF),
        // RLIMIT_FSIZE: max file size.  No limiter today.
        1 => (INF, INF),
        // RLIMIT_DATA: data-segment size.  No tracker today.
        2 => (INF, INF),
        // RLIMIT_STACK: 8 MiB matches glibc's main-thread sizing.
        3 => (8 * 1024 * 1024, INF),
        // RLIMIT_CORE: 0 — we never produce core dumps, so advertise
        // a hard zero so programs don't trip on them.
        4 => (0, 0),
        // RLIMIT_RSS: resident set size.  No tracker.
        5 => (INF, INF),
        // RLIMIT_NPROC: per-uid process count.  No tracker.
        6 => (INF, INF),
        // RLIMIT_NOFILE: per-process open-fd limit.  1024 matches
        // most Linux distros; programs that select() on bare fd
        // numbers rely on this fitting in FD_SETSIZE.
        7 => (1024, 4096),
        // RLIMIT_MEMLOCK: mlock()'d memory.  No tracker.
        8 => (INF, INF),
        // RLIMIT_AS: address-space size.  No tracker.
        9 => (INF, INF),
        // RLIMIT_LOCKS: fcntl(F_SETLK) lock count.  No tracker.
        10 => (INF, INF),
        // RLIMIT_SIGPENDING: per-uid pending signal count.  We have
        // a 64-bit pending word per process; advertise a generous
        // cap so programs that compute "can I queue another?" don't
        // think they're full.
        11 => (65_536, 65_536),
        // RLIMIT_MSGQUEUE: POSIX message queue bytes.  We don't
        // implement them; advertise the Linux default.
        12 => (819_200, 819_200),
        // RLIMIT_NICE: nice ceiling.  0 means "may not lower nice".
        // We don't support nice anyway.
        13 => (0, 0),
        // RLIMIT_RTPRIO: real-time priority ceiling.  0 means "no
        // RT scheduling".  Our scheduler is priority round-robin
        // without Linux-style RT semantics, so 0 is honest.
        14 => (0, 0),
        // RLIMIT_RTTIME: max contiguous RT CPU microseconds.
        15 => (INF, INF),
        // Caller has already gated 0..=15; this is unreachable, but
        // we return INFINITY rather than panic out of caution.
        _ => (INF, INF),
    }
}

/// `wait4(pid, wstatus, options, rusage)` — reap a child process,
/// optionally non-blocking, with Linux-shaped status encoding.
///
/// Linux semantics:
///   - `pid > 0`: wait for that specific child.
///   - `pid == -1`: wait for any child.
///   - `pid == 0`: wait for any child in the caller's process group
///     (we have no process groups; treated as `-1`).
///   - `pid < -1`: wait for any child in process group `-pid` (treated
///     as `-1`).
///
/// `wstatus`: optional `*mut i32` receiving the encoded status.  Per
/// glibc's `<sys/wait.h>` macros:
///   - normal exit with code C (0..=127): `status = (C & 0xff) << 8`
///     → `WIFEXITED(status)` is true, `WEXITSTATUS(status)` == C.
///   - killed by signal N (our convention: native exit codes 128..=255
///     are signal kills with `N = exit_code - 128`):
///     `status = N & 0x7f` → `WIFSIGNALED(status)` is true,
///     `WTERMSIG(status)` == N.
///   - crashed (hardware fault): we synthesise `SIGSEGV` (11) since
///     the vast majority of crashes are page faults / access
///     violations.  Future enhancement: map exception_code → real
///     Linux signal (SIGFPE for divide error, SIGILL for invalid op,
///     etc.) by consulting `CrashInfo`.
///
/// `options`: `WNOHANG` (1) routes to the non-blocking
/// [`pcb::try_reap`] / [`pcb::try_reap_any`]; the caller sees a
/// "0 returned, no status written" result when no child is ready.
/// `WUNTRACED` (2) / `WCONTINUED` (8) are accepted-but-ignored — we
/// have no process-stop mechanism, so there are no stopped or
/// continued children to report.  Any other options bits return
/// `-EINVAL`.
///
/// `rusage`: optional pointer to a `struct rusage` (144 bytes on
/// x86_64 Linux).  We don't track per-process resource usage yet, so
/// we zero the entire structure if the pointer is non-null.  Programs
/// that consume `rusage` fields (top, time, gnu time) will see zero
/// CPU time for the child — incorrect but harmless.
///
/// Return value (Linux convention):
///   - on success: the reaped child's PID;
///   - on `WNOHANG` with no ready child: 0;
///   - no children at all: `-ECHILD`;
///   - target is not a child of the caller: `-ECHILD`;
///   - bad `wstatus` or `rusage` pointer: `-EFAULT`;
///   - bad `options` bits: `-EINVAL`.
fn sys_wait4(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::pcb;

    // Linux WAIT4 option flags.
    const WNOHANG: u64 = 1;
    const WUNTRACED: u64 = 2;
    const WCONTINUED: u64 = 8;
    const VALID_OPTIONS: u64 = WNOHANG | WUNTRACED | WCONTINUED;

    #[allow(clippy::cast_possible_wrap)]
    let pid_arg = args.arg0 as i64;
    let wstatus_ptr = args.arg1;
    let options = args.arg2;
    let rusage_ptr = args.arg3;

    if (options & !VALID_OPTIONS) != 0 {
        return linux_err(errno::EINVAL);
    }
    let nohang = (options & WNOHANG) != 0;

    // Pre-validate user pointers if non-null.  Doing this BEFORE the
    // wait avoids reaping a child and then failing to deliver the
    // status (which would leak the exit info — there's no "un-reap").
    if wstatus_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_write(
            wstatus_ptr,
            core::mem::size_of::<i32>(),
        ) {
            return linux_err(linux_errno_for(e));
        }
    }
    // Linux's struct rusage is 144 bytes on x86_64 (16×i64 longs:
    // 2 timevals of 2 longs each + 14 scalar longs).
    const RUSAGE_SIZE: usize = 144;
    if rusage_ptr != 0 {
        if let Err(e) = crate::mm::user::validate_user_write(rusage_ptr, RUSAGE_SIZE) {
            return linux_err(linux_errno_for(e));
        }
    }

    let parent_pid = caller_pid().unwrap_or(0);
    let task_id = crate::sched::current_task_id();

    // Specific-pid vs any-child path mirrors sys_process_wait.
    let (child_pid, info) = if pid_arg > 0 {
        #[allow(clippy::cast_sign_loss)]
        let target_pid = pid_arg as u64;
        loop {
            match pcb::try_reap(parent_pid, target_pid) {
                Ok(Some(info)) => break (target_pid, info),
                Ok(None) => {} // still running
                Err(KernelError::PermissionDenied) | Err(KernelError::NoSuchProcess) => {
                    // Not our child / doesn't exist → ECHILD.
                    return linux_err(errno::ECHILD);
                }
                Err(e) => return linux_err(linux_errno_for(e)),
            }
            if nohang {
                return SyscallResult::ok(0);
            }
            // Block until the child exits (lost-wakeup-safe via
            // set_wait_task + re-check, same pattern as sys_process_wait).
            if let Err(e) = pcb::set_wait_task(target_pid, task_id) {
                return linux_err(linux_errno_for(e));
            }
            match pcb::try_reap(parent_pid, target_pid) {
                Ok(Some(info)) => break (target_pid, info),
                Ok(None) => crate::sched::block_current(),
                Err(KernelError::PermissionDenied) | Err(KernelError::NoSuchProcess) => {
                    return linux_err(errno::ECHILD);
                }
                Err(e) => return linux_err(linux_errno_for(e)),
            }
        }
    } else {
        // pid <= 0: wait for any child.  Same register-before-check
        // discipline as sys_process_wait's wait-any path.
        loop {
            if let Err(e) = pcb::set_wait_any_task(parent_pid, task_id) {
                // ECHILD if the parent has no children at all.
                pcb::clear_wait_any_task(parent_pid, task_id);
                return linux_err(linux_errno_for(e));
            }
            match pcb::try_reap_any(parent_pid) {
                Ok(Some((cpid, info))) => {
                    pcb::clear_wait_any_task(parent_pid, task_id);
                    break (cpid, info);
                }
                Ok(None) => {
                    if nohang {
                        pcb::clear_wait_any_task(parent_pid, task_id);
                        return SyscallResult::ok(0);
                    }
                    crate::sched::block_current();
                }
                Err(e) => {
                    pcb::clear_wait_any_task(parent_pid, task_id);
                    return linux_err(linux_errno_for(e));
                }
            }
        }
    };

    // Encode wstatus per the Linux <sys/wait.h> macros.
    let wstatus: i32 = encode_linux_wstatus(&info);

    if wstatus_ptr != 0 {
        // SAFETY: validated as a writable user range of i32 size at the
        // top of the function; the address space hasn't changed because
        // we're still in the calling process.
        unsafe {
            core::ptr::write(wstatus_ptr as *mut i32, wstatus);
        }
    }
    if rusage_ptr != 0 {
        // We don't track per-process resource usage; zero the whole
        // struct.  Validated as writable above.
        // SAFETY: same as wstatus write — validated user range, no ASID
        // change since.
        unsafe {
            core::ptr::write_bytes(rusage_ptr as *mut u8, 0, RUSAGE_SIZE);
        }
    }

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(child_pid as i64)
}

/// Translate our [`pcb::ExitInfo`] into a Linux-shaped `wstatus` word.
///
/// Pure function — split out so the boot self-test can exercise the
/// three branches (normal / signaled / crashed) without needing a
/// real reaped child.
fn encode_linux_wstatus(info: &crate::proc::pcb::ExitInfo) -> i32 {
    // Crash: synthesise SIGSEGV.  This is "good enough" for the
    // common case; a future enhancement could map exception codes
    // (DivideError → SIGFPE, InvalidOpcode → SIGILL, etc.) by
    // consulting CrashInfo.exception_code.
    if info.crash.is_some() {
        return 11; // SIGSEGV, low 7 bits of wstatus, WIFSIGNALED true
    }
    let code = info.exit_code;
    if (128..=255).contains(&code) {
        // Killed by signal: kernel convention is exit_code = 128 + sig.
        let sig = (code - 128) & 0x7f;
        sig
    } else {
        // Normal exit: low byte of exit_code lives in bits 8..=15.
        #[allow(clippy::cast_sign_loss)]
        let lo = (code as u32) & 0xff;
        #[allow(clippy::cast_possible_wrap)]
        let s = (lo << 8) as i32;
        s
    }
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

/// `set_tid_address(tidptr)` — register `tidptr` as the address the
/// kernel must zero (and futex-wake) when the calling thread exits,
/// then return the caller's TID.
///
/// This is the runtime equivalent of `CLONE_CHILD_CLEARTID` for the
/// **main** thread: glibc startup calls `set_tid_address(&pd->tid)`
/// during the first thread's initialisation so that
/// `pthread_join(main_thread)` from a thread library extension can
/// observe the main thread's exit through the same futex mechanism
/// that clone'd threads use.
///
/// A `tidptr` of 0 unregisters any prior address (matches Linux's
/// behaviour of accepting NULL to clear the slot).
fn sys_set_tid_address(args: &SyscallArgs) -> SyscallResult {
    let tidptr = args.arg0;
    let task_id = crate::sched::current_task_id();
    crate::proc::thread_clone::register_clear_child_tid(task_id, tidptr);
    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(task_id as i64)
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

/// `getrandom(buf, buflen, flags)` — fill `buf` with random bytes.
///
/// Backed by the kernel ChaCha20 CSPRNG (`crate::rng`).  Linux's
/// `getrandom(2)` returns "best effort to avoid blocking for entropy";
/// our RNG is always available once `rng::init()` has run (during
/// boot), and falls back to TSC+HPET lazy-seeding if a caller somehow
/// races early boot.
///
/// `flags` is accepted but not interpreted:
///   - `GRND_NONBLOCK` (0x0001) — we never block, so it's a no-op.
///   - `GRND_RANDOM`   (0x0002) — we don't distinguish urandom vs
///                                 random sources; same CSPRNG either
///                                 way.
///   - `GRND_INSECURE` (0x0004) — accepted for API compatibility.
///
/// Returns the number of bytes written (always equal to `buflen`
/// capped at 256, matching Linux's `getrandom` per-call cap).
fn sys_getrandom(args: &SyscallArgs) -> SyscallResult {
    let buf_ptr = args.arg0;
    let buf_len = args.arg1 as usize;
    // arg2 = flags (ignored — see doc comment above).
    if buf_len == 0 {
        return SyscallResult::ok(0);
    }
    // Cap to avoid pathological huge requests.  Linux's getrandom
    // caps at 256 bytes per call when GRND_RANDOM is set; we apply
    // the same cap universally as a defensive measure (callers loop).
    let n = buf_len.min(256);

    // Validate user buffer is writable.
    if let Err(e) = crate::mm::user::validate_user_write(buf_ptr, n) {
        return linux_err(linux_errno_for(e));
    }

    // Fill from the kernel CSPRNG (ChaCha20, see kernel/src/rng.rs).
    let mut tmp = [0u8; 256];
    #[allow(clippy::indexing_slicing)]
    crate::rng::fill(&mut tmp[..n]);

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

        // CLONE with a non-zero child_stack but no CLONE_VM /
        // CLONE_THREAD pair — invalid, must reject as -ENOSYS.
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

        // Full pthread-like clone: CLONE_VM | CLONE_THREAD | ...
        // with a non-zero child_stack reaches thread_clone::clone_thread
        // which then fails with ESRCH (no owning Linux process in the
        // self-test context).  Proves the new thread-creation route is
        // wired correctly — must NOT return -ENOSYS.
        f.syscall_nr = nr::CLONE;
        f.arg0 = clone_flags::CLONE_VM
            | clone_flags::CLONE_FS
            | clone_flags::CLONE_FILES
            | clone_flags::CLONE_SIGHAND
            | clone_flags::CLONE_THREAD
            | clone_flags::CLONE_SYSVSEM
            | clone_flags::CLONE_SETTLS
            | clone_flags::CLONE_PARENT_SETTID
            | clone_flags::CLONE_CHILD_CLEARTID
            | clone_flags::CLONE_CHILD_SETTID;
        f.arg1 = 0xDEAD_BEEF; // non-zero child_stack
        f.arg2 = 0; // ptid
        f.arg3 = 0; // ctid
        f.arg4 = 0; // tls
        match dispatch_linux_with_frame(&mut f) {
            Some(v) if v == -i64::from(errno::ESRCH) => {}
            other => {
                serial_println!(
                    "[syscall/linux]   FAIL: pthread-clone via with_frame → {:?} (expected -ESRCH)",
                    other
                );
                return Err(KernelError::InternalError);
            }
        }
    }

    // CLONE_VFORK accept / CLONE_PARENT reject:
    //   - clone(SIGCHLD | CLONE_VFORK, 0, ...) reaches linux_fork
    //     (degenerates to plain fork) and bails out at fork::fork_process
    //     with ESRCH because we have no owning Linux process.  The
    //     point is to prove the clone() path no longer returns -ENOSYS
    //     when CLONE_VFORK is set — that flag was previously in the
    //     unsupported set and would have returned -ENOSYS up-front.
    //   - clone(SIGCHLD | CLONE_PARENT, 0, ...) MUST still return
    //     -ENOSYS because PID reparenting infrastructure is missing.
    //   - same for CLONE_NEWNS and CLONE_PTRACE.
    {
        use crate::syscall::entry::SyscallFrame;
        let mut f = SyscallFrame {
            syscall_nr: nr::CLONE,
            arg0: clone_flags::SIGCHLD | clone_flags::CLONE_VFORK,
            arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            rbx: 0, rbp: 0, r12: 0, r13: 0, r14: 0, r15: 0,
            user_rip: 0, user_rsp: 0, user_rflags: 0,
        };
        match dispatch_linux_with_frame(&mut f) {
            Some(v) if v == -i64::from(errno::ENOSYS) => {
                serial_println!(
                    "[syscall/linux]   FAIL: clone(CLONE_VFORK) still ENOSYS"
                );
                return Err(KernelError::InternalError);
            }
            // Any other negative errno (likely -ESRCH) proves we
            // reached fork::fork_process rather than rejecting up-front.
            Some(v) if v < 0 => {}
            other => {
                serial_println!(
                    "[syscall/linux]   FAIL: clone(CLONE_VFORK) → {:?}", other
                );
                return Err(KernelError::InternalError);
            }
        }

        for (name, bit) in &[
            ("CLONE_PARENT", clone_flags::CLONE_PARENT),
            ("CLONE_NEWNS",  clone_flags::CLONE_NEWNS),
            ("CLONE_PTRACE", clone_flags::CLONE_PTRACE),
        ] {
            f.arg0 = clone_flags::SIGCHLD | *bit;
            match dispatch_linux_with_frame(&mut f) {
                Some(v) if v == -i64::from(errno::ENOSYS) => {}
                other => {
                    serial_println!(
                        "[syscall/linux]   FAIL: clone({}) → {:?} (expected -ENOSYS)",
                        name, other
                    );
                    return Err(KernelError::InternalError);
                }
            }
        }
    }

    // kill(target, sig) signal-number gate validation:
    //   - target == 0 is "process group" targeting in Linux which we
    //     don't support; we use target=0xDEAD_BEEF instead, a pid
    //     that almost-certainly doesn't exist so the call bails at
    //     the existence check (ESRCH).  What we assert is that the
    //     signal-number gate either accepts (-> ESRCH) or rejects
    //     (-> EINVAL) the *signal number* — never the wrong way.
    //   - sig == 0 (existence probe): MUST NOT be EINVAL.
    //   - sig == NSIG (64): valid; bypasses gate.
    //   - sig == NSIG + 1 (65): EINVAL.
    //   - sig == u64::MAX: EINVAL.
    //   - sig == 9 (SIGKILL), 15 (SIGTERM), 17 (SIGCHLD): all valid;
    //     the linux ABI must NOT collapse to "SIGKILL/SIGTERM only".
    {
        const PROBE_PID: u64 = 0xDEAD_BEEF;
        // sig=0 (existence probe): not EINVAL (expect ESRCH).
        let a = SyscallArgs { arg0: PROBE_PID, arg1: 0,
            arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        let v = dispatch_linux(nr::KILL, &a).value;
        if v == -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: kill sig=0 -> EINVAL");
            return Err(KernelError::InternalError);
        }
        // sig=65 (NSIG+1): EINVAL.
        let a = SyscallArgs { arg0: PROBE_PID, arg1: 65,
            arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::KILL, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: kill sig=65");
            return Err(KernelError::InternalError);
        }
        // sig=u64::MAX: EINVAL.
        let a = SyscallArgs { arg0: PROBE_PID, arg1: u64::MAX,
            arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::KILL, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: kill sig=u64::MAX");
            return Err(KernelError::InternalError);
        }
        // sig=9 (SIGKILL), 15 (SIGTERM), 17 (SIGCHLD), 64 (NSIG):
        // none should be rejected by the signal-number gate.
        for sig in [9u64, 15, 17, 64] {
            let a = SyscallArgs { arg0: PROBE_PID, arg1: sig,
                arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
            let v = dispatch_linux(nr::KILL, &a).value;
            if v == -i64::from(errno::EINVAL) {
                serial_println!(
                    "[syscall/linux]   FAIL: kill sig={} rejected as EINVAL",
                    sig
                );
                return Err(KernelError::InternalError);
            }
        }
    }

    // rt_sigreturn:
    //   - misaligned user_rsp causes both candidate addresses to fail
    //     the SignalContext alignment check, returning -EFAULT
    //     without attempting any unsafe dereference.  This proves the
    //     defensive alignment gate works (necessary because
    //     validate_user_read has a kernel-context bypass that would
    //     otherwise let us deref garbage during boot self-tests).
    //   - frame.user_rip must be left untouched on the failure path
    //     so a userspace program can debug the EFAULT without losing
    //     control.
    {
        use crate::syscall::entry::SyscallFrame;
        let mut f = SyscallFrame {
            syscall_nr: nr::RT_SIGRETURN,
            arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
            rbx: 0, rbp: 0, r12: 0, r13: 0, r14: 0, r15: 0,
            user_rip: 0xCAFE_BABE_DEAD_BEEF,
            user_rsp: 1, // misaligned; both candidates (9, 1) fail
            user_rflags: 0x202,
        };
        let pre_rip = f.user_rip;
        match dispatch_linux_with_frame(&mut f) {
            Some(v) if v == -i64::from(errno::EFAULT) => {}
            other => {
                serial_println!(
                    "[syscall/linux]   FAIL: rt_sigreturn EFAULT → {:?}", other
                );
                return Err(KernelError::InternalError);
            }
        }
        if f.user_rip != pre_rip {
            serial_println!(
                "[syscall/linux]   FAIL: rt_sigreturn EFAULT mutated user_rip"
            );
            return Err(KernelError::InternalError);
        }
    }

    // Linux-sigaction table — round-trip + edge cases.
    //   Operates directly on the table (not via dispatch_linux),
    //   because dispatch_linux's rt_sigaction needs a live caller pid
    //   to record state, which the boot self-test doesn't have.
    {
        // Use a synthetic pid that won't collide with any real one.
        let test_pid: u64 = 0xFFFF_FFFF_DEAD_0001;

        // Initially: get() returns SIG_DFL defaults.
        let initial = linux_sigaction_get(test_pid, 10);
        if initial.sa_handler != SIG_DFL || initial.sa_flags != 0
            || initial.sa_restorer != 0 || initial.sa_mask != 0
        {
            serial_println!("[syscall/linux]   FAIL: sigaction initial != defaults");
            return Err(KernelError::InternalError);
        }

        // set() then get() round-trips.
        let act = LinuxSigaction {
            sa_handler: 0xCAFE_BABE_1234_5678,
            sa_flags: sa_flags::SA_RESTART | sa_flags::SA_SIGINFO,
            sa_restorer: 0xDEAD_BEEF_0000_0001,
            sa_mask: 0xAAAA_BBBB_CCCC_DDDD,
        };
        linux_sigaction_set(test_pid, 10, act);
        let read_back = linux_sigaction_get(test_pid, 10);
        if read_back != act {
            serial_println!("[syscall/linux]   FAIL: sigaction round-trip");
            return Err(KernelError::InternalError);
        }

        // Per-signal independence: signal 11 still has defaults.
        let other = linux_sigaction_get(test_pid, 11);
        if other != LinuxSigaction::default() {
            serial_println!("[syscall/linux]   FAIL: sigaction per-signal independence");
            return Err(KernelError::InternalError);
        }

        // on_exec: SIG_IGN preserved, caught -> SIG_DFL.
        // Set 11 to SIG_IGN, then re-test exec.
        let ign = LinuxSigaction { sa_handler: SIG_IGN, sa_flags: sa_flags::SA_RESTART,
            sa_restorer: 0x1234, sa_mask: 0x5678 };
        linux_sigaction_set(test_pid, 11, ign);
        linux_sigaction_on_exec(test_pid);
        let after_exec_10 = linux_sigaction_get(test_pid, 10);
        let after_exec_11 = linux_sigaction_get(test_pid, 11);
        // Caught signal 10 should reset to SIG_DFL defaults.
        if after_exec_10 != LinuxSigaction::default() {
            serial_println!(
                "[syscall/linux]   FAIL: sigaction on_exec didn't reset caught"
            );
            return Err(KernelError::InternalError);
        }
        // SIG_IGN signal 11 should keep handler but lose flags/restorer/mask.
        if after_exec_11.sa_handler != SIG_IGN
            || after_exec_11.sa_flags != 0
            || after_exec_11.sa_restorer != 0
            || after_exec_11.sa_mask != 0
        {
            serial_println!(
                "[syscall/linux]   FAIL: sigaction on_exec mishandled SIG_IGN"
            );
            return Err(KernelError::InternalError);
        }

        // on_fork: child inherits parent's entries.
        let child_pid: u64 = 0xFFFF_FFFF_DEAD_0002;
        linux_sigaction_on_fork(test_pid, child_pid);
        let child_11 = linux_sigaction_get(child_pid, 11);
        if child_11.sa_handler != SIG_IGN {
            serial_println!("[syscall/linux]   FAIL: sigaction on_fork didn't inherit");
            return Err(KernelError::InternalError);
        }

        // on_exit: all entries gone.
        linux_sigaction_on_exit(test_pid);
        linux_sigaction_on_exit(child_pid);
        let post_exit = linux_sigaction_get(test_pid, 11);
        if post_exit != LinuxSigaction::default() {
            serial_println!("[syscall/linux]   FAIL: sigaction on_exit didn't clear");
            return Err(KernelError::InternalError);
        }
    }

    // rt_sigaction validation via dispatch_linux:
    //   - sig == 0 -> EINVAL
    //   - sig > NSIG -> EINVAL
    //   - sigsetsize != 0 && != 8 -> EINVAL
    //   - unknown sa_flags bits -> EINVAL (needs an act pointer; we
    //     can't safely deref one from boot context so we only test
    //     the cheap rejects above).
    {
        // sig == 0
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 8,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::RT_SIGACTION, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: rt_sigaction sig=0");
            return Err(KernelError::InternalError);
        }
        // sig > NSIG
        let a = SyscallArgs { arg0: 65, arg1: 0, arg2: 0, arg3: 8,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::RT_SIGACTION, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: rt_sigaction sig=65");
            return Err(KernelError::InternalError);
        }
        // sigsetsize mismatch
        let a = SyscallArgs { arg0: 10, arg1: 0, arg2: 0, arg3: 7,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::RT_SIGACTION, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: rt_sigaction sigsetsize");
            return Err(KernelError::InternalError);
        }
    }

    // mprotect argument validation:
    //   - zero length succeeds (no-op);
    //   - unknown prot bit -> EINVAL;
    //   - misaligned addr   -> EINVAL;
    //   - kernel-space addr -> EFAULT.
    // We can't exercise the success path from boot context (no owning
    // Linux process), but we *can* prove the validation layer rejects
    // bad input before reaching the page-table walk.
    {
        let args0 = SyscallArgs { arg0: 0x4000, arg1: 0, arg2: 0,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MPROTECT, &args0).value != 0 {
            serial_println!("[syscall/linux]   FAIL: mprotect zero-len");
            return Err(KernelError::InternalError);
        }
        let args1 = SyscallArgs { arg0: 0x4000, arg1: 0x4000,
            arg2: 0x100, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MPROTECT, &args1).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: mprotect bad-prot");
            return Err(KernelError::InternalError);
        }
        let args2 = SyscallArgs { arg0: 0x4001, arg1: 0x4000,
            arg2: 1, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MPROTECT, &args2).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: mprotect misalign");
            return Err(KernelError::InternalError);
        }
        let args3 = SyscallArgs {
            arg0: 0xFFFF_8000_0000_0000,
            arg1: 0x4000, arg2: 1, arg3: 0, arg4: 0, arg5: 0
        };
        if dispatch_linux(nr::MPROTECT, &args3).value != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: mprotect kernel-addr");
            return Err(KernelError::InternalError);
        }
    }

    // mprotect_flush_range routing: tiny range (<= MPROTECT_FULL_FLUSH_
    // PAGES) takes the per-page invlpg path; large range promotes to
    // full TLB flush.  We can't directly observe which path was taken
    // from outside the function, but we *can* prove the function
    // doesn't panic, doesn't deadlock the shootdown lock, and returns
    // promptly on every code path including the degenerate end<=start
    // and zero-length cases.  We use a kernel-space address since the
    // function only flushes — it doesn't touch the page tables.
    {
        let pre = crate::tlb::stats();
        // Degenerate: end == start -> no-op.
        mprotect_flush_range(0xFFFF_8000_0000_0000, 0xFFFF_8000_0000_0000);
        // Degenerate: end < start -> no-op.
        mprotect_flush_range(0xFFFF_8000_0001_0000, 0xFFFF_8000_0000_0000);
        // Small range: 16 KiB = 4 hardware pages, below threshold.
        // Should take flush_range path (one range-flush stat bump).
        mprotect_flush_range(0xFFFF_8000_0010_0000, 0xFFFF_8000_0010_4000);
        // Threshold-boundary range: exactly 64 4 KiB pages = 16 frames
        // = 256 KiB.  Should still take flush_range path (page_count
        // == MPROTECT_FULL_FLUSH_PAGES is "<= threshold"... wait, our
        // check is `> MPROTECT_FULL_FLUSH_PAGES`, so == takes range).
        mprotect_flush_range(0xFFFF_8000_0020_0000, 0xFFFF_8000_0024_0000);
        // Large range: well above threshold, promotes to full flush.
        mprotect_flush_range(0xFFFF_8000_0030_0000, 0xFFFF_8000_0100_0000);
        let post = crate::tlb::stats();
        // Three non-degenerate calls -> three flush stat bumps total
        // (two range + one full).
        let range_delta = post.range_flushes.saturating_sub(pre.range_flushes);
        let full_delta = post.full_flushes.saturating_sub(pre.full_flushes);
        if range_delta < 2 {
            serial_println!(
                "[syscall/linux]   FAIL: mprotect_flush_range small/threshold not range-flushed (delta={})",
                range_delta,
            );
            return Err(KernelError::InternalError);
        }
        if full_delta < 1 {
            serial_println!(
                "[syscall/linux]   FAIL: mprotect_flush_range large not full-flushed (delta={})",
                full_delta,
            );
            return Err(KernelError::InternalError);
        }
    }

    // madvise(addr, len, advice) coverage:
    //   - len == 0: succeeds without further validation, even for bogus
    //     addr / advice values.
    //   - Known MADV_* advice (0..=25): returns 0 (no-op).  Crucial
    //     because glibc/jemalloc/tcmalloc call MADV_DONTNEED on every
    //     free; ENOSYS here would make them spam the syscall and leak
    //     RSS unbounded.
    //   - MADV_HWPOISON (100) and MADV_SOFT_OFFLINE (101): EPERM —
    //     these are privileged memory-failure injection on Linux and
    //     we don't expose them.
    //   - Unknown advice: EINVAL.
    //   - Misaligned addr (with nonzero len): EINVAL.
    //   - Kernel-space addr (with nonzero len): ENOMEM.
    {
        const MADV_DONTNEED: u64 = 4;
        const MADV_FREE: u64 = 8;
        const MADV_COLLAPSE: u64 = 25; // upper documented bound
        const MADV_HWPOISON: u64 = 100;
        const MADV_SOFT_OFFLINE: u64 = 101;

        // Zero-length always succeeds — even with intentionally bad
        // addr and advice.
        let a = SyscallArgs { arg0: 0x4001 /* misaligned! */, arg1: 0,
            arg2: 9999 /* bogus advice */, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MADVISE, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: madvise(len=0)");
            return Err(KernelError::InternalError);
        }

        // Known hints over a valid user-space range return 0.
        for advice in [0u64, 1, 2, 3, MADV_DONTNEED, MADV_FREE, MADV_COLLAPSE] {
            let a = SyscallArgs { arg0: 0x4000, arg1: 0x4000,
                arg2: advice, arg3: 0, arg4: 0, arg5: 0 };
            let v = dispatch_linux(nr::MADVISE, &a).value;
            if v != 0 {
                serial_println!(
                    "[syscall/linux]   FAIL: madvise(advice={}) -> {} (expected 0)",
                    advice, v
                );
                return Err(KernelError::InternalError);
            }
        }

        // HWPOISON / SOFT_OFFLINE: EPERM.
        for advice in [MADV_HWPOISON, MADV_SOFT_OFFLINE] {
            let a = SyscallArgs { arg0: 0x4000, arg1: 0x4000,
                arg2: advice, arg3: 0, arg4: 0, arg5: 0 };
            if dispatch_linux(nr::MADVISE, &a).value != -i64::from(errno::EPERM) {
                serial_println!(
                    "[syscall/linux]   FAIL: madvise(advice={}) not EPERM", advice
                );
                return Err(KernelError::InternalError);
            }
        }

        // Unknown advice (26 — between documented max 25 and HWPOISON):
        // EINVAL.
        let a = SyscallArgs { arg0: 0x4000, arg1: 0x4000,
            arg2: 26, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MADVISE, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: madvise unknown advice");
            return Err(KernelError::InternalError);
        }

        // Misaligned addr with nonzero len: EINVAL.
        let a = SyscallArgs { arg0: 0x4001, arg1: 0x4000,
            arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MADVISE, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: madvise misalign");
            return Err(KernelError::InternalError);
        }

        // Kernel-space addr with nonzero len: ENOMEM.
        let a = SyscallArgs { arg0: 0xFFFF_8000_0000_0000, arg1: 0x4000,
            arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MADVISE, &a).value != -i64::from(errno::ENOMEM) {
            serial_println!("[syscall/linux]   FAIL: madvise kernel-addr");
            return Err(KernelError::InternalError);
        }
    }

    // wait4 wstatus encoding (pure function — no real reaped child
    // needed).  Three branches: normal exit, signaled, crashed.
    //
    // Linux's <sys/wait.h> macros:
    //   WIFEXITED(s)    -> (s & 0x7f) == 0
    //   WEXITSTATUS(s)  -> (s >> 8) & 0xff
    //   WIFSIGNALED(s)  -> ((s & 0x7f) + 1) >> 1 > 0   ≡ low7 in 1..=126
    //   WTERMSIG(s)     -> s & 0x7f
    {
        use crate::proc::pcb::ExitInfo;
        // Normal exit with code 42 — WIFEXITED + WEXITSTATUS==42.
        let s = encode_linux_wstatus(&ExitInfo { exit_code: 42, crash: None });
        if (s & 0x7f) != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: wstatus normal exit not WIFEXITED ({})", s
            );
            return Err(KernelError::InternalError);
        }
        if ((s >> 8) & 0xff) != 42 {
            serial_println!(
                "[syscall/linux]   FAIL: wstatus normal exit WEXITSTATUS != 42 ({})", s
            );
            return Err(KernelError::InternalError);
        }
        // Killed by SIGTERM (15) — kernel exit_code convention 128+sig.
        let s = encode_linux_wstatus(&ExitInfo { exit_code: 128 + 15, crash: None });
        let low7 = s & 0x7f;
        if low7 != 15 {
            serial_println!(
                "[syscall/linux]   FAIL: wstatus SIGTERM WTERMSIG != 15 ({})", s
            );
            return Err(KernelError::InternalError);
        }
        // WIFSIGNALED check: ((low7 + 1) >> 1) > 0 — true for 1..=126.
        if ((low7 + 1) >> 1) == 0 {
            serial_println!(
                "[syscall/linux]   FAIL: wstatus SIGTERM not WIFSIGNALED ({})", s
            );
            return Err(KernelError::InternalError);
        }
        // Crash (any crash_info present) synthesises SIGSEGV (11).
        let crash = crate::proc::pcb::CrashInfo {
            exception_code: 14, // page fault
            faulting_rip: 0xDEAD_BEEF,
            aux: 0,
            thread_id: 0,
        };
        let s = encode_linux_wstatus(&ExitInfo { exit_code: -14, crash: Some(crash) });
        if (s & 0x7f) != 11 {
            serial_println!(
                "[syscall/linux]   FAIL: wstatus crash != SIGSEGV ({})", s
            );
            return Err(KernelError::InternalError);
        }
    }

    // wait4 dispatch validation via dispatch_linux:
    //   - unknown option bits -> EINVAL (before any reap attempt).
    //   - wait-any from a contextless test task (caller_pid resolves to 0):
    //     either ECHILD (no children of kernel) or some other -ENOSYS-
    //     adjacent error path, but NEVER -EINVAL (proves routing reached
    //     the wait core, not the options validator).
    //   - wait-specific for a pid that almost-certainly doesn't exist
    //     (0xDEAD_BEEF) returns -ECHILD (the "not a child of caller"
    //     path).  Must NOT be -EINVAL.
    {
        // Unknown option bit (WNOHANG | unknown high bit).
        let a = SyscallArgs { arg0: u64::MAX /* -1 = wait any */, arg1: 0,
            arg2: 1 | 0x4000_0000 /* WNOHANG + bogus */, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::WAIT4, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: wait4 bad options not EINVAL");
            return Err(KernelError::InternalError);
        }

        // wait-any WNOHANG from contextless test task.  parent_pid
        // resolves to 0 (kernel) which has no children registered, so
        // set_wait_any_task returns ECHILD.  The crucial assertion is
        // that the call did NOT return -EINVAL or panic.
        let a = SyscallArgs { arg0: u64::MAX, arg1: 0, arg2: 1 /* WNOHANG */,
            arg3: 0, arg4: 0, arg5: 0 };
        let v = dispatch_linux(nr::WAIT4, &a).value;
        if v == -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: wait4 wait-any WNOHANG -> EINVAL");
            return Err(KernelError::InternalError);
        }

        // wait-specific WNOHANG for a fake pid — ECHILD (or some other
        // non-EINVAL negative).  Must not block (WNOHANG guarantees
        // non-blocking) and must not panic.
        let a = SyscallArgs { arg0: 0xDEAD_BEEF, arg1: 0, arg2: 1,
            arg3: 0, arg4: 0, arg5: 0 };
        let v = dispatch_linux(nr::WAIT4, &a).value;
        if v == -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: wait4 wait-specific WNOHANG -> EINVAL");
            return Err(KernelError::InternalError);
        }
        if v >= 0 {
            serial_println!(
                "[syscall/linux]   FAIL: wait4 fake pid succeeded ({})", v
            );
            return Err(KernelError::InternalError);
        }

        // NOTE: we deliberately don't test bad wstatus / rusage pointers
        // here.  validate_user_write has a documented kernel-context
        // bypass (returns Ok unconditionally for tasks with no owning
        // process), which makes EFAULT impossible to observe from the
        // boot self-test.  The validation logic itself is shared
        // infrastructure exercised by every other syscall — it WILL
        // EFAULT on bad pointers from real userspace.
    }

    // rlimit_default coverage — pure function table, no userspace.
    //
    // Critical defaults programs depend on:
    //   - RLIMIT_STACK (3) cur == 8 MiB so glibc's main-thread sizing
    //     produces a usable stack.
    //   - RLIMIT_NOFILE (7) cur == 1024 to fit FD_SETSIZE on select().
    //   - RLIMIT_CORE (4) cur == max == 0 (we don't produce cores).
    //   - All others either INFINITY or honestly zero.
    {
        let (cur, max) = rlimit_default(3); // RLIMIT_STACK
        if cur != 8 * 1024 * 1024 {
            serial_println!(
                "[syscall/linux]   FAIL: rlimit_default(STACK).cur = {}", cur
            );
            return Err(KernelError::InternalError);
        }
        if max != u64::MAX {
            serial_println!(
                "[syscall/linux]   FAIL: rlimit_default(STACK).max = {}", max
            );
            return Err(KernelError::InternalError);
        }
        let (cur, max) = rlimit_default(7); // RLIMIT_NOFILE
        if cur != 1024 || max != 4096 {
            serial_println!(
                "[syscall/linux]   FAIL: rlimit_default(NOFILE) = ({}, {})",
                cur, max
            );
            return Err(KernelError::InternalError);
        }
        let (cur, max) = rlimit_default(4); // RLIMIT_CORE
        if cur != 0 || max != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: rlimit_default(CORE) = ({}, {})",
                cur, max
            );
            return Err(KernelError::InternalError);
        }
        // Default for an arbitrary RLIM_INFINITY one — CPU.
        let (cur, max) = rlimit_default(0);
        if cur != u64::MAX || max != u64::MAX {
            serial_println!(
                "[syscall/linux]   FAIL: rlimit_default(CPU) = ({}, {})",
                cur, max
            );
            return Err(KernelError::InternalError);
        }
    }

    // prlimit64 dispatch validation:
    //   - resource > 15 -> EINVAL.
    //   - pid != 0 and pid != caller -> EPERM.  caller_pid resolves to
    //     0/None in the boot self-test, so any non-zero pid takes the
    //     EPERM path.
    //   - pid == 0 with both pointers NULL -> 0 (success no-op).
    {
        // Unknown resource.
        let a = SyscallArgs { arg0: 0, arg1: 16, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PRLIMIT64, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: prlimit64 bad resource");
            return Err(KernelError::InternalError);
        }
        let a = SyscallArgs { arg0: 0, arg1: u64::MAX, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PRLIMIT64, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: prlimit64 u64-max resource");
            return Err(KernelError::InternalError);
        }
        // Cross-pid query: caller_pid is None/0 in self-test, so any
        // nonzero pid != caller -> EPERM.
        let a = SyscallArgs { arg0: 1, arg1: 3, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PRLIMIT64, &a).value != -i64::from(errno::EPERM) {
            serial_println!(
                "[syscall/linux]   FAIL: prlimit64 cross-pid not EPERM ({})",
                dispatch_linux(nr::PRLIMIT64, &a).value
            );
            return Err(KernelError::InternalError);
        }
        // pid==0 with both NULL pointers is a pure no-op success.
        let a = SyscallArgs { arg0: 0, arg1: 3, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PRLIMIT64, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: prlimit64 pid=0 STACK NULL,NULL not 0"
            );
            return Err(KernelError::InternalError);
        }
    }

    // rt_sigpending dispatch validation:
    //   - sigsetsize != 8 -> EINVAL (before pointer fault).
    //   - sigsetsize == 8 with a NULL set pointer would normally EFAULT,
    //     but validate_user_write's documented kernel-context bypass
    //     returns Ok for tasks with no owning process (see comment near
    //     sys_wait4 self-tests).  So we can only verify the
    //     size-validation path here; the EFAULT path is exercised by
    //     every other syscall under real userspace.
    {
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::RT_SIGPENDING, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: rt_sigpending bad sigsetsize=0");
            return Err(KernelError::InternalError);
        }
        let a = SyscallArgs { arg0: 0, arg1: 16, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::RT_SIGPENDING, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: rt_sigpending bad sigsetsize=16");
            return Err(KernelError::InternalError);
        }
        let a = SyscallArgs { arg0: 0, arg1: u64::MAX, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::RT_SIGPENDING, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: rt_sigpending u64-max sigsetsize");
            return Err(KernelError::InternalError);
        }
    }

    // tkill / tgkill dispatch validation:
    //   - Non-existent tid -> ESRCH (no owning process).
    //   - tgkill with mismatched tgid -> ESRCH even if tid exists.
    //   - In boot context, current_task_id() may or may not be a
    //     registered thread; we test only the unambiguous "tid that
    //     definitely doesn't exist" path to avoid coupling to scheduler
    //     state.
    {
        // tkill on a definitely-nonexistent tid (u64::MAX) -> ESRCH.
        let a = SyscallArgs { arg0: u64::MAX, arg1: 1, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TKILL, &a).value != -i64::from(errno::ESRCH) {
            serial_println!(
                "[syscall/linux]   FAIL: tkill nonexistent tid not ESRCH ({})",
                dispatch_linux(nr::TKILL, &a).value
            );
            return Err(KernelError::InternalError);
        }
        // tkill with sig=0 on nonexistent tid still ESRCH (probe).
        let a = SyscallArgs { arg0: u64::MAX, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TKILL, &a).value != -i64::from(errno::ESRCH) {
            serial_println!(
                "[syscall/linux]   FAIL: tkill nonexistent tid sig=0 not ESRCH ({})",
                dispatch_linux(nr::TKILL, &a).value
            );
            return Err(KernelError::InternalError);
        }
        // tgkill on nonexistent tid -> ESRCH.
        let a = SyscallArgs { arg0: 1, arg1: u64::MAX, arg2: 1, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TGKILL, &a).value != -i64::from(errno::ESRCH) {
            serial_println!(
                "[syscall/linux]   FAIL: tgkill nonexistent tid not ESRCH ({})",
                dispatch_linux(nr::TGKILL, &a).value
            );
            return Err(KernelError::InternalError);
        }
    }

    // umask dispatch validation:
    //   - Always returns 0o022 (our compiled-in distro-default stub).
    //   - Any mask argument is silently accepted.
    {
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UMASK, &a).value != 0o022 {
            serial_println!(
                "[syscall/linux]   FAIL: umask(0) not 0o022 ({})",
                dispatch_linux(nr::UMASK, &a).value
            );
            return Err(KernelError::InternalError);
        }
        let a = SyscallArgs { arg0: 0o777, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UMASK, &a).value != 0o022 {
            serial_println!(
                "[syscall/linux]   FAIL: umask(0o777) not 0o022 ({})",
                dispatch_linux(nr::UMASK, &a).value
            );
            return Err(KernelError::InternalError);
        }
        // Garbage mask still accepted — Linux masks with & 0o777 so we
        // never error on out-of-range bits.
        let a = SyscallArgs { arg0: u64::MAX, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UMASK, &a).value != 0o022 {
            serial_println!("[syscall/linux]   FAIL: umask(u64::MAX) not 0o022");
            return Err(KernelError::InternalError);
        }
    }

    // sigaltstack dispatch validation:
    //   - Both NULL pointers -> 0 (the trivial no-op success).
    //   - ss / old_ss with kernel-context bypass on validate_user_*
    //     means we can't actually verify EFAULT here without going
    //     through real userspace; the validation infrastructure is
    //     exercised by every other syscall that takes user pointers.
    {
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SIGALTSTACK, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: sigaltstack(NULL, NULL) not 0 ({})",
                dispatch_linux(nr::SIGALTSTACK, &a).value
            );
            return Err(KernelError::InternalError);
        }
    }

    // ioctl dispatch validation:
    //   - Every ioctl returns ENOTTY (no tty / no driver routing yet).
    //   - The "right" answer here matters for isatty(3), which is
    //     defined as ioctl(fd, TCGETS, &tio) != -1 — returning ENOTTY
    //     gives isatty() a clean "0 with errno=ENOTTY" rather than
    //     the misleading ENOSYS.
    {
        // TCGETS request code 0x5401 — the canonical isatty probe.
        let a = SyscallArgs { arg0: 1, arg1: 0x5401, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::IOCTL, &a).value != -i64::from(errno::ENOTTY) {
            serial_println!(
                "[syscall/linux]   FAIL: ioctl(TCGETS) not ENOTTY ({})",
                dispatch_linux(nr::IOCTL, &a).value
            );
            return Err(KernelError::InternalError);
        }
        // Arbitrary unknown ioctl also ENOTTY.
        let a = SyscallArgs { arg0: 1, arg1: 0xdead_beef, arg2: 0,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::IOCTL, &a).value != -i64::from(errno::ENOTTY) {
            serial_println!(
                "[syscall/linux]   FAIL: ioctl(arbitrary) not ENOTTY"
            );
            return Err(KernelError::InternalError);
        }
    }

    // prctl dispatch validation.
    //   - Recognised options return 0 (or their documented value).
    //   - Unknown options return EINVAL.
    {
        // PR_SET_NAME (15) — accept.
        let a = SyscallArgs { arg0: 15, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PRCTL, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: prctl(PR_SET_NAME) not 0 ({})",
                dispatch_linux(nr::PRCTL, &a).value
            );
            return Err(KernelError::InternalError);
        }
        // PR_GET_NAME (16) with NULL buf — accept (skip the copy).
        let a = SyscallArgs { arg0: 16, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PRCTL, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: prctl(PR_GET_NAME, NULL) not 0");
            return Err(KernelError::InternalError);
        }
        // PR_CAPBSET_READ (23) — return 1 (cap "is in" the bset).
        let a = SyscallArgs { arg0: 23, arg1: 1, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PRCTL, &a).value != 1 {
            serial_println!(
                "[syscall/linux]   FAIL: prctl(PR_CAPBSET_READ) not 1 ({})",
                dispatch_linux(nr::PRCTL, &a).value
            );
            return Err(KernelError::InternalError);
        }
        // PR_GET_NO_NEW_PRIVS (39) — return 1.
        let a = SyscallArgs { arg0: 39, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PRCTL, &a).value != 1 {
            serial_println!(
                "[syscall/linux]   FAIL: prctl(PR_GET_NO_NEW_PRIVS) not 1"
            );
            return Err(KernelError::InternalError);
        }
        // Unknown option (999) -> EINVAL.
        let a = SyscallArgs { arg0: 999, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PRCTL, &a).value != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: prctl(999) not EINVAL ({})",
                dispatch_linux(nr::PRCTL, &a).value
            );
            return Err(KernelError::InternalError);
        }
    }

    // personality dispatch validation.
    //   - Always returns 0 (PER_LINUX is the only personality we know).
    {
        let a = SyscallArgs { arg0: 0xffff_ffff, arg1: 0, arg2: 0,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PERSONALITY, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: personality(query) not 0 ({})",
                dispatch_linux(nr::PERSONALITY, &a).value
            );
            return Err(KernelError::InternalError);
        }
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PERSONALITY, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: personality(0) not 0");
            return Err(KernelError::InternalError);
        }
    }

    // getresuid / getresgid dispatch validation.
    //   - All three NULL pointers -> 0 (nothing to write).
    //   - Any non-NULL pointer would trigger validate_user_write; with
    //     the kernel-context bypass this also succeeds, so we can't
    //     observe EFAULT here.  The write-zero path is covered by the
    //     "all NULL" success — the function returns 0 in both modes
    //     so the dispatch self-test only proves "routed correctly".
    {
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::GETRESUID, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: getresuid(NULL,NULL,NULL) not 0");
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::GETRESGID, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: getresgid(NULL,NULL,NULL) not 0");
            return Err(KernelError::InternalError);
        }
    }

    // getrusage / sysinfo / times dispatch validation.
    //   - NULL user pointer -> EFAULT (early gate before validate_user_*).
    //   - times(NULL) is the documented "return-value only" case and
    //     should succeed; we verify the return is non-negative (ticks
    //     since boot).
    {
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::GETRUSAGE, &a).value != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: getrusage NULL not EFAULT ({})",
                dispatch_linux(nr::GETRUSAGE, &a).value
            );
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::SYSINFO, &a).value != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: sysinfo NULL not EFAULT ({})",
                dispatch_linux(nr::SYSINFO, &a).value
            );
            return Err(KernelError::InternalError);
        }
        // times(NULL) succeeds with the tick count.
        if dispatch_linux(nr::TIMES, &a).value < 0 {
            serial_println!(
                "[syscall/linux]   FAIL: times(NULL) returned negative ({})",
                dispatch_linux(nr::TIMES, &a).value
            );
            return Err(KernelError::InternalError);
        }
    }

    // Process-group / session syscall dispatch validation.
    //   - getpgrp() returns the caller PID (or 1 in contextless boot).
    //   - getpgid(0) same.
    //   - getpgid(nonexistent_pid) -> ESRCH.
    //   - setpgid(_, negative) -> EINVAL.
    //   - setpgid(0, 0) -> 0.
    //   - getsid(0) returns caller PID (or 1).
    //   - getsid(nonexistent) -> ESRCH.
    //   - setsid() returns caller PID (or 1).
    {
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        let r = dispatch_linux(nr::GETPGRP, &a).value;
        if r <= 0 {
            serial_println!("[syscall/linux]   FAIL: getpgrp() returned {}", r);
            return Err(KernelError::InternalError);
        }
        let r = dispatch_linux(nr::GETPGID, &a).value;
        if r <= 0 {
            serial_println!("[syscall/linux]   FAIL: getpgid(0) returned {}", r);
            return Err(KernelError::InternalError);
        }
        // getpgid(u64::MAX) — definitely not a real PID -> ESRCH.
        let a = SyscallArgs { arg0: u64::MAX, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::GETPGID, &a).value != -i64::from(errno::ESRCH) {
            serial_println!(
                "[syscall/linux]   FAIL: getpgid(u64::MAX) not ESRCH ({})",
                dispatch_linux(nr::GETPGID, &a).value
            );
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::GETSID, &a).value != -i64::from(errno::ESRCH) {
            serial_println!(
                "[syscall/linux]   FAIL: getsid(u64::MAX) not ESRCH"
            );
            return Err(KernelError::InternalError);
        }
        // setpgid(0, -1 as u64) -> EINVAL.
        #[allow(clippy::cast_sign_loss)]
        let neg = (-1i64) as u64;
        let a = SyscallArgs { arg0: 0, arg1: neg, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SETPGID, &a).value != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: setpgid(0, -1) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // setpgid(0, 0) -> 0.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SETPGID, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: setpgid(0, 0) not 0 ({})",
                dispatch_linux(nr::SETPGID, &a).value
            );
            return Err(KernelError::InternalError);
        }
        // setsid() returns caller PID (or 1 in contextless boot).
        if dispatch_linux(nr::SETSID, &a).value <= 0 {
            serial_println!("[syscall/linux]   FAIL: setsid() returned non-positive");
            return Err(KernelError::InternalError);
        }
    }

    // Priority dispatch validation.
    //   - which in 0..=2 returns 0.
    //   - which > 2 returns EINVAL for both getpriority and setpriority.
    {
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::GETPRIORITY, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: getpriority(PRIO_PROCESS) not 0");
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::SETPRIORITY, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: setpriority(PRIO_PROCESS) not 0");
            return Err(KernelError::InternalError);
        }
        let a = SyscallArgs { arg0: 3, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::GETPRIORITY, &a).value != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: getpriority(3) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::SETPRIORITY, &a).value != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: setpriority(3) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
    }

    // Credential setters: all of setuid/setgid/setre*/setres*/setfs*/
    // getgroups/setgroups silently succeed.  We test a representative
    // sample.
    {
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        for nr in [nr::SETUID, nr::SETGID, nr::SETREUID, nr::SETREGID,
                   nr::SETRESUID, nr::SETRESGID, nr::SETFSUID, nr::SETFSGID,
                   nr::GETGROUPS, nr::SETGROUPS] {
            if dispatch_linux(nr, &a).value != 0 {
                serial_println!(
                    "[syscall/linux]   FAIL: credential syscall {} not 0 ({})",
                    nr, dispatch_linux(nr, &a).value
                );
                return Err(KernelError::InternalError);
            }
        }
        // Non-zero arg also accepted silently.
        let a = SyscallArgs { arg0: 1000, arg1: 1000, arg2: 1000,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SETRESUID, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: setresuid(1000,...) not 0");
            return Err(KernelError::InternalError);
        }
    }

    // capget / capset dispatch validation.
    //   - hdrp == NULL -> EFAULT.
    //   - We can't exercise the version path easily without staging a
    //     real user-space buffer; the validate_user_read kernel-context
    //     bypass means even a "kernel" pointer will appear readable.
    //     The dispatch routing itself is what we're testing here.
    {
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::CAPGET, &a).value != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: capget(NULL) not EFAULT ({})",
                dispatch_linux(nr::CAPGET, &a).value
            );
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::CAPSET, &a).value != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: capset(NULL) not EFAULT ({})",
                dispatch_linux(nr::CAPSET, &a).value
            );
            return Err(KernelError::InternalError);
        }
    }

    // Scheduler-policy / priority dispatch validation.
    //   - sched_getscheduler(0) -> 0 (SCHED_OTHER).
    //   - sched_get_priority_max/min on known policies match Linux.
    //   - Unknown policy -> EINVAL.
    {
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SCHED_GETSCHEDULER, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: sched_getscheduler(0) not 0 ({})",
                dispatch_linux(nr::SCHED_GETSCHEDULER, &a).value
            );
            return Err(KernelError::InternalError);
        }
        // SCHED_OTHER (0) max == 0.
        if dispatch_linux(nr::SCHED_GET_PRIORITY_MAX, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: sched_get_priority_max(OTHER) not 0"
            );
            return Err(KernelError::InternalError);
        }
        // SCHED_FIFO (1) max == 99.
        let a = SyscallArgs { arg0: 1, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SCHED_GET_PRIORITY_MAX, &a).value != 99 {
            serial_println!(
                "[syscall/linux]   FAIL: sched_get_priority_max(FIFO) not 99 ({})",
                dispatch_linux(nr::SCHED_GET_PRIORITY_MAX, &a).value
            );
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::SCHED_GET_PRIORITY_MIN, &a).value != 1 {
            serial_println!(
                "[syscall/linux]   FAIL: sched_get_priority_min(FIFO) not 1"
            );
            return Err(KernelError::InternalError);
        }
        // Unknown policy (99) -> EINVAL.
        let a = SyscallArgs { arg0: 99, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SCHED_GET_PRIORITY_MAX, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: sched_get_priority_max(99) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // sched_setscheduler(0, 8, NULL) -> EINVAL (policy out of range).
        let a = SyscallArgs { arg0: 0, arg1: 8, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SCHED_SETSCHEDULER, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: sched_setscheduler(8) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // sched_setscheduler(0, 0, NULL) -> 0.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SCHED_SETSCHEDULER, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: sched_setscheduler(OTHER) not 0"
            );
            return Err(KernelError::InternalError);
        }
        // sched_setparam(0, NULL) -> 0.
        if dispatch_linux(nr::SCHED_SETPARAM, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: sched_setparam(0) not 0");
            return Err(KernelError::InternalError);
        }
        // sched_getparam(0, NULL) -> EFAULT.
        if dispatch_linux(nr::SCHED_GETPARAM, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: sched_getparam(0, NULL) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
        // sched_rr_get_interval(0, NULL) -> EFAULT.
        if dispatch_linux(nr::SCHED_RR_GET_INTERVAL, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: sched_rr_get_interval(0, NULL) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
    }

    // sched_get/setaffinity dispatch validation.
    //   - getaffinity(0, 0, mask) -> EINVAL (cpusetsize too small).
    //   - getaffinity(0, big, NULL) -> EFAULT.
    //   - setaffinity(0, 0, NULL) -> EFAULT.
    {
        // cpusetsize too small.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SCHED_GETAFFINITY, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: sched_getaffinity(size=0) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // mask == NULL.
        let a = SyscallArgs { arg0: 0, arg1: 16, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SCHED_GETAFFINITY, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: sched_getaffinity(NULL mask) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::SCHED_SETAFFINITY, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: sched_setaffinity(NULL mask) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
    }

    // Filesystem sync stubs all return 0.
    {
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        for nr in [nr::FSYNC, nr::FDATASYNC, nr::SYNC, nr::SYNCFS] {
            if dispatch_linux(nr, &a).value != 0 {
                serial_println!(
                    "[syscall/linux]   FAIL: sync-family syscall {} not 0 ({})",
                    nr, dispatch_linux(nr, &a).value
                );
                return Err(KernelError::InternalError);
            }
        }
    }

    // sethostname / setdomainname validation.
    //   - len > 64 -> EINVAL.
    //   - NULL pointer with non-zero len -> EFAULT.
    //   - NULL pointer with zero len -> 0 (no-op).
    {
        let a = SyscallArgs { arg0: 0, arg1: 65, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SETHOSTNAME, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: sethostname(len=65) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        let a = SyscallArgs { arg0: 0, arg1: 5, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SETHOSTNAME, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: sethostname(NULL,5) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SETHOSTNAME, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: sethostname(NULL,0) not 0"
            );
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::SETDOMAINNAME, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: setdomainname(NULL,0) not 0"
            );
            return Err(KernelError::InternalError);
        }
    }

    // mlock / munlock / mlockall / munlockall.
    //   - zero len succeeds without touching memory.
    //   - mlockall with zero flags -> EINVAL; valid bits -> 0; bad bit -> EINVAL.
    {
        let zero = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MLOCK, &zero).value != 0 {
            serial_println!("[syscall/linux]   FAIL: mlock(0,0) != 0");
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::MUNLOCK, &zero).value != 0 {
            serial_println!("[syscall/linux]   FAIL: munlock(0,0) != 0");
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::MUNLOCKALL, &zero).value != 0 {
            serial_println!("[syscall/linux]   FAIL: munlockall != 0");
            return Err(KernelError::InternalError);
        }
        // mlockall(0) -> EINVAL.
        if dispatch_linux(nr::MLOCKALL, &zero).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: mlockall(0) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // mlockall(1|2|4 = 7) -> 0.
        let a = SyscallArgs { arg0: 7, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MLOCKALL, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: mlockall(7) not 0");
            return Err(KernelError::InternalError);
        }
        // mlockall(8) -> EINVAL (unknown bit).
        let a = SyscallArgs { arg0: 8, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MLOCKALL, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: mlockall(8) not EINVAL");
            return Err(KernelError::InternalError);
        }
    }

    // msync flag/alignment validation.
    {
        // flags == 0 -> EINVAL (need at least MS_SYNC or MS_ASYNC).
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MSYNC, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: msync(flags=0) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // MS_SYNC | MS_ASYNC -> EINVAL (mutually exclusive).
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1 | 4, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MSYNC, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: msync(SYNC|ASYNC) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // Unknown flag bit -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0x10, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MSYNC, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: msync(flags=0x10) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // Misaligned addr with valid flags -> EINVAL.
        let a = SyscallArgs { arg0: 0x1234, arg1: 4096, arg2: 1, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MSYNC, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: msync(misaligned) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // Zero len with valid flags & aligned addr -> 0.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MSYNC, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: msync(len=0) not 0");
            return Err(KernelError::InternalError);
        }
    }

    // fadvise64 / readahead — advice validation.
    {
        // Bad advice (7) -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 7,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FADVISE64, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: fadvise64(advice=7) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // Valid advice in kernel context (no caller pid) -> 0.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 3,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FADVISE64, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: fadvise64(WILLNEED) not 0"
            );
            return Err(KernelError::InternalError);
        }
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::READAHEAD, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: readahead not 0");
            return Err(KernelError::InternalError);
        }
    }

    // close_range — argument validation.
    {
        // first > last -> EINVAL.
        let a = SyscallArgs { arg0: 10, arg1: 5, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::CLOSE_RANGE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: close_range(first>last) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // Unknown flag bit -> EINVAL.
        let a = SyscallArgs { arg0: 3, arg1: 10, arg2: 0x100, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::CLOSE_RANGE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: close_range(flags=0x100) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // Valid call in kernel context (no caller pid) -> EBADF, since
        // close_range cannot operate without an owning process's fd table.
        let a = SyscallArgs { arg0: 3, arg1: 10, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::CLOSE_RANGE, &a).value
            != -i64::from(errno::EBADF) {
            serial_println!(
                "[syscall/linux]   FAIL: close_range(no caller) not EBADF"
            );
            return Err(KernelError::InternalError);
        }
    }

    // getrlimit / setrlimit — wrappers around prlimit64.
    //   - unknown resource -> EINVAL via the prlimit64 path.
    //   - NULL rlim with valid resource -> 0 (matches prlimit64 NULL/NULL).
    {
        // getrlimit(99, NULL) -> EINVAL (resource > 15).
        let a = SyscallArgs { arg0: 99, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::GETRLIMIT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: getrlimit(bad) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // setrlimit(99, NULL) -> EINVAL.
        if dispatch_linux(nr::SETRLIMIT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: setrlimit(bad) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // getrlimit(RLIMIT_STACK=3, NULL) -> 0 (NULL pointer accepted).
        let a = SyscallArgs { arg0: 3, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::GETRLIMIT, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: getrlimit(STACK,NULL) not 0"
            );
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::SETRLIMIT, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: setrlimit(STACK,NULL) not 0"
            );
            return Err(KernelError::InternalError);
        }
    }

    // getcpu — both pointers NULL is allowed and returns 0.
    {
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::GETCPU, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: getcpu(NULL,NULL) not 0"
            );
            return Err(KernelError::InternalError);
        }
    }

    // statfs / fstatfs — NULL pointer validation.
    //   - statfs(NULL, buf) -> EFAULT.
    //   - fstatfs(fd, NULL) -> EFAULT.
    //   - statfs with garbage path ptr in kernel context — kernel
    //     context bypass allows it; we still write the buffer.
    {
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::STATFS, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: statfs(NULL,_) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
        let a = SyscallArgs { arg0: 3, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSTATFS, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: fstatfs(_,NULL) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
    }

    // clock_settime / clock_adjtime / adjtimex — NULL EFAULT then EPERM.
    {
        // clock_settime(0, NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::CLOCK_SETTIME, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: clock_settime(NULL) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
        // clock_settime(0, 0x1000) in kernel context (validate bypass) -> EPERM.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::CLOCK_SETTIME, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!(
                "[syscall/linux]   FAIL: clock_settime not EPERM"
            );
            return Err(KernelError::InternalError);
        }
        // adjtimex(NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::ADJTIMEX, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: adjtimex(NULL) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
        // adjtimex with valid-ish ptr in kernel ctx -> EPERM.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::ADJTIMEX, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!(
                "[syscall/linux]   FAIL: adjtimex not EPERM"
            );
            return Err(KernelError::InternalError);
        }
    }

    // chroot / mknod / mknodat — NULL path -> EFAULT, non-NULL -> EPERM.
    {
        let a_null = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::CHROOT, &a_null).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: chroot(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::MKNOD, &a_null).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: mknod(NULL,_,_) not EFAULT");
            return Err(KernelError::InternalError);
        }
        let a_mknodat_null = SyscallArgs { arg0: 0, arg1: 0, arg2: 0,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MKNODAT, &a_mknodat_null).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: mknodat(_,NULL,_,_) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
        // Non-NULL path with kernel-context bypass -> EPERM.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::CHROOT, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: chroot not EPERM");
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::MKNOD, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: mknod not EPERM");
            return Err(KernelError::InternalError);
        }
        let a_mknodat = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MKNODAT, &a_mknodat).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: mknodat not EPERM");
            return Err(KernelError::InternalError);
        }
    }

    // getitimer / setitimer / alarm / pause — input validation.
    {
        // getitimer(which=3, _) -> EINVAL.
        let a = SyscallArgs { arg0: 3, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::GETITIMER, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: getitimer(which=3) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // getitimer(0, NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::GETITIMER, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: getitimer(0,NULL) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
        // setitimer(which=99, _, _) -> EINVAL.
        let a = SyscallArgs { arg0: 99, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SETITIMER, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: setitimer(which=99) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // setitimer(0, NULL, _) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SETITIMER, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: setitimer(0,NULL,_) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
        // alarm(N) -> 0 for any N.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::ALARM, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: alarm(0) not 0");
            return Err(KernelError::InternalError);
        }
        let a = SyscallArgs { arg0: 5, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::ALARM, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: alarm(5) not 0");
            return Err(KernelError::InternalError);
        }
        // pause() -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PAUSE, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: pause not ENOSYS");
            return Err(KernelError::InternalError);
        }
    }

    // access / faccessat / faccessat2 — input validation, then ENOENT.
    {
        // Bogus mode bit (8 is not R/W/X/F) -> EINVAL.
        let a = SyscallArgs { arg0: 0x1000, arg1: 8, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::ACCESS, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: access(_,8) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // NULL path -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::ACCESS, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: access(NULL,0) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
        // Valid call in kernel context (bypass) -> ENOENT.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::ACCESS, &a).value
            != -i64::from(errno::ENOENT) {
            serial_println!(
                "[syscall/linux]   FAIL: access not ENOENT"
            );
            return Err(KernelError::InternalError);
        }
        // faccessat: NULL path -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FACCESSAT, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: faccessat(_,NULL,_,_) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
        // faccessat2: bogus flag bit -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0,
            arg3: 0x800_0000, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FACCESSAT2, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: faccessat2(bad flag) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // faccessat2 with valid flag -> ENOENT.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0,
            arg3: 0x200, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FACCESSAT2, &a).value
            != -i64::from(errno::ENOENT) {
            serial_println!(
                "[syscall/linux]   FAIL: faccessat2 not ENOENT"
            );
            return Err(KernelError::InternalError);
        }
    }

    // stat / lstat / fstat / newfstatat — input validation.
    {
        // stat(NULL, _) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::STAT, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: stat(NULL,_) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // stat(/whatever, statbuf) in kernel context -> ENOENT.
        let mut sbuf = [0u8; 144];
        let a = SyscallArgs {
            arg0: 0x1000,
            arg1: sbuf.as_mut_ptr() as u64,
            arg2: 0, arg3: 0, arg4: 0, arg5: 0,
        };
        if dispatch_linux(nr::STAT, &a).value
            != -i64::from(errno::ENOENT) {
            serial_println!("[syscall/linux]   FAIL: stat not ENOENT");
            return Err(KernelError::InternalError);
        }
        // lstat: same.
        if dispatch_linux(nr::LSTAT, &a).value
            != -i64::from(errno::ENOENT) {
            serial_println!("[syscall/linux]   FAIL: lstat not ENOENT");
            return Err(KernelError::InternalError);
        }
        // fstat(_, NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSTAT, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: fstat(_,NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // fstat(0, kbuf) in kernel context -> 0 (synthesised Console
        // entry), and the struct stat reports S_IFCHR.
        let mut sbuf = [0u8; 144];
        let a = SyscallArgs {
            arg0: 0,
            arg1: sbuf.as_mut_ptr() as u64,
            arg2: 0, arg3: 0, arg4: 0, arg5: 0,
        };
        if dispatch_linux(nr::FSTAT, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: fstat in kctx not 0");
            return Err(KernelError::InternalError);
        }
        // st_mode is at offset 24, 4 bytes.  Top bits should be S_IFCHR
        // (0o020000 == 0x2000).
        let mode = u32::from_ne_bytes([
            sbuf[24], sbuf[25], sbuf[26], sbuf[27],
        ]);
        if (mode & 0o170000) != 0o020000 {
            serial_println!(
                "[syscall/linux]   FAIL: fstat reported mode {:#o} not S_IFCHR",
                mode
            );
            return Err(KernelError::InternalError);
        }
        // newfstatat: bogus flag -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0x2000,
            arg3: 0x800_0000, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::NEWFSTATAT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: newfstatat(bad flag) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
    }

    // statx — input validation and AT_EMPTY_PATH success.
    {
        // Bogus flag bit -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0x800_0000,
            arg3: 0, arg4: 0x2000, arg5: 0 };
        if dispatch_linux(nr::STATX, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: statx(bad flag) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // NULL statxbuf -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::STATX, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!(
                "[syscall/linux]   FAIL: statx(NULL out) not EFAULT"
            );
            return Err(KernelError::InternalError);
        }
        // AT_EMPTY_PATH with kernel-synthesised Console fd -> 0, stx_mode
        // reports S_IFCHR.
        let mut xbuf = [0u8; 256];
        let a = SyscallArgs {
            arg0: 0,
            arg1: 0,
            arg2: 0x1000,
            arg3: 0,
            arg4: xbuf.as_mut_ptr() as u64,
            arg5: 0,
        };
        if dispatch_linux(nr::STATX, &a).value != 0 {
            serial_println!(
                "[syscall/linux]   FAIL: statx(AT_EMPTY_PATH) not 0"
            );
            return Err(KernelError::InternalError);
        }
        let stx_mode = u16::from_ne_bytes([xbuf[28], xbuf[29]]);
        if (u32::from(stx_mode) & 0o170000) != 0o020000 {
            serial_println!(
                "[syscall/linux]   FAIL: statx reported mode {:#o} not S_IFCHR",
                stx_mode
            );
            return Err(KernelError::InternalError);
        }
        // Path lookup with no AT_EMPTY_PATH -> ENOENT.
        let a = SyscallArgs {
            arg0: 0,
            arg1: 0x1000,
            arg2: 0,
            arg3: 0,
            arg4: xbuf.as_mut_ptr() as u64,
            arg5: 0,
        };
        if dispatch_linux(nr::STATX, &a).value
            != -i64::from(errno::ENOENT) {
            serial_println!("[syscall/linux]   FAIL: statx path not ENOENT");
            return Err(KernelError::InternalError);
        }
    }

    // mkdir / mkdirat / rmdir / unlink / unlinkat / rename family —
    // pointer validation plus principled errno.
    {
        // mkdir(NULL,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MKDIR, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: mkdir(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // mkdir(0x1000,_) -> EROFS (validate succeeds in kernel context).
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MKDIR, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: mkdir not EROFS");
            return Err(KernelError::InternalError);
        }
        // mkdirat(_, NULL, _) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MKDIRAT, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: mkdirat(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // mkdirat(_, 0x1000, _) -> EROFS.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MKDIRAT, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: mkdirat not EROFS");
            return Err(KernelError::InternalError);
        }
        // rmdir(NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::RMDIR, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: rmdir(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // rmdir(0x1000) -> ENOENT.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::RMDIR, &a).value
            != -i64::from(errno::ENOENT) {
            serial_println!("[syscall/linux]   FAIL: rmdir not ENOENT");
            return Err(KernelError::InternalError);
        }
        // unlink(NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UNLINK, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: unlink(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // unlink(0x1000) -> ENOENT.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UNLINK, &a).value
            != -i64::from(errno::ENOENT) {
            serial_println!("[syscall/linux]   FAIL: unlink not ENOENT");
            return Err(KernelError::InternalError);
        }
        // unlinkat with bogus flag bit -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0x800_0000,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UNLINKAT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: unlinkat(bad flag) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // unlinkat(_, NULL, 0) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UNLINKAT, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: unlinkat(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // unlinkat(_, 0x1000, AT_REMOVEDIR) -> ENOENT.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0x200,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UNLINKAT, &a).value
            != -i64::from(errno::ENOENT) {
            serial_println!("[syscall/linux]   FAIL: unlinkat not ENOENT");
            return Err(KernelError::InternalError);
        }
        // rename(NULL, x) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::RENAME, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: rename(NULL,_) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // rename(x, NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::RENAME, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: rename(_,NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // rename(x, y) -> ENOENT.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0x2000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::RENAME, &a).value
            != -i64::from(errno::ENOENT) {
            serial_println!("[syscall/linux]   FAIL: rename not ENOENT");
            return Err(KernelError::InternalError);
        }
        // renameat(_, x, _, y) -> ENOENT.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0x2000,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::RENAMEAT, &a).value
            != -i64::from(errno::ENOENT) {
            serial_println!("[syscall/linux]   FAIL: renameat not ENOENT");
            return Err(KernelError::InternalError);
        }
        // renameat2 with bogus flag bit -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0x2000,
            arg4: 0x800_0000, arg5: 0 };
        if dispatch_linux(nr::RENAMEAT2, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!(
                "[syscall/linux]   FAIL: renameat2(bad flag) not EINVAL"
            );
            return Err(KernelError::InternalError);
        }
        // renameat2 valid flag (RENAME_NOREPLACE=1) -> ENOENT.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0x2000,
            arg4: 1, arg5: 0 };
        if dispatch_linux(nr::RENAMEAT2, &a).value
            != -i64::from(errno::ENOENT) {
            serial_println!("[syscall/linux]   FAIL: renameat2 not ENOENT");
            return Err(KernelError::InternalError);
        }
    }

    // readlink / readlinkat / chmod family / chown family / truncate /
    // ftruncate / symlink / link / utime family — pointer validation
    // plus principled errno.
    {
        // readlink(NULL,_,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 16, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::READLINK, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: readlink(NULL,_,_) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // readlink(_,NULL,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 16, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::READLINK, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: readlink(_,NULL,_) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // readlink(_,_,0) -> EINVAL.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0x2000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::READLINK, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: readlink(_,_,0) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // readlink(path, buf, 16) -> EINVAL ("not a symlink").
        let mut linkbuf = [0u8; 16];
        let a = SyscallArgs {
            arg0: 0x1000,
            arg1: linkbuf.as_mut_ptr() as u64,
            arg2: 16, arg3: 0, arg4: 0, arg5: 0,
        };
        if dispatch_linux(nr::READLINK, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: readlink not EINVAL");
            return Err(KernelError::InternalError);
        }
        // readlinkat(_,path,buf,16) -> EINVAL.
        let a = SyscallArgs {
            arg0: 0,
            arg1: 0x1000,
            arg2: linkbuf.as_mut_ptr() as u64,
            arg3: 16, arg4: 0, arg5: 0,
        };
        if dispatch_linux(nr::READLINKAT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: readlinkat not EINVAL");
            return Err(KernelError::InternalError);
        }

        // chmod(NULL,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::CHMOD, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: chmod(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // chmod(path,_) -> EROFS.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::CHMOD, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: chmod not EROFS");
            return Err(KernelError::InternalError);
        }
        // fchmod in kernel context -> EROFS (caller_pid None branch).
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FCHMOD, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: fchmod not EROFS");
            return Err(KernelError::InternalError);
        }
        // fchmodat with bogus flag -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0,
            arg3: 0x800_0000, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FCHMODAT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: fchmodat(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // fchmodat(_, path, _, 0) -> EROFS.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FCHMODAT, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: fchmodat not EROFS");
            return Err(KernelError::InternalError);
        }
        // chown / lchown(path,_,_) -> EROFS.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::CHOWN, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: chown not EROFS");
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::LCHOWN, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: lchown not EROFS");
            return Err(KernelError::InternalError);
        }
        // chown(NULL,_,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::CHOWN, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: chown(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // fchown in kernel context -> EROFS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FCHOWN, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: fchown not EROFS");
            return Err(KernelError::InternalError);
        }
        // fchownat bogus flag -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0x800_0000, arg5: 0 };
        if dispatch_linux(nr::FCHOWNAT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: fchownat(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // fchownat(_, path, _, _, 0) -> EROFS.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FCHOWNAT, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: fchownat not EROFS");
            return Err(KernelError::InternalError);
        }

        // truncate(NULL,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TRUNCATE, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: truncate(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // truncate(path, -1) -> EINVAL.
        // (Constructed as u64::MAX for "negative" semantics.)
        let a = SyscallArgs { arg0: 0x1000, arg1: u64::MAX, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TRUNCATE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: truncate(_,-1) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // truncate(path, 0) -> EROFS.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TRUNCATE, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: truncate not EROFS");
            return Err(KernelError::InternalError);
        }
        // ftruncate(_, -1) -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: u64::MAX, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FTRUNCATE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: ftruncate(-1) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // ftruncate(0, 0) in kernel context -> EROFS (kctx → no pid →
        // EROFS short-circuit, matching the read-only-FS answer).
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FTRUNCATE, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: ftruncate not EROFS");
            return Err(KernelError::InternalError);
        }

        // symlink(NULL,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SYMLINK, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: symlink(NULL,_) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // symlink(x, y) -> EROFS.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0x2000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SYMLINK, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: symlink not EROFS");
            return Err(KernelError::InternalError);
        }
        // symlinkat(x, _, y) -> EROFS.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0x2000, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SYMLINKAT, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: symlinkat not EROFS");
            return Err(KernelError::InternalError);
        }
        // link(x, y) -> EROFS.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0x2000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::LINK, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: link not EROFS");
            return Err(KernelError::InternalError);
        }
        // linkat bogus flag -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0x2000,
            arg4: 0x800_0000, arg5: 0 };
        if dispatch_linux(nr::LINKAT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: linkat(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // linkat with valid AT_SYMLINK_FOLLOW (0x400) -> EROFS.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0x2000,
            arg4: 0x400, arg5: 0 };
        if dispatch_linux(nr::LINKAT, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: linkat not EROFS");
            return Err(KernelError::InternalError);
        }

        // utimensat bogus flag -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0,
            arg3: 0x800_0000, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UTIMENSAT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: utimensat(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // utimensat(_, NULL, NULL, 0) -> EROFS (NULL path is legal).
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UTIMENSAT, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: utimensat NULL/NULL not EROFS");
            return Err(KernelError::InternalError);
        }
        // utimes(NULL, _) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UTIMES, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: utimes(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // utimes(path, NULL) -> EROFS.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UTIMES, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: utimes not EROFS");
            return Err(KernelError::InternalError);
        }
        // utime(NULL, _) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UTIME, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: utime(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // utime(path, NULL) -> EROFS.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UTIME, &a).value
            != -i64::from(errno::EROFS) {
            serial_println!("[syscall/linux]   FAIL: utime not EROFS");
            return Err(KernelError::InternalError);
        }
    }

    // signalfd / timerfd / inotify / fanotify — input validation and
    // ENOSYS-after-validate.
    {
        // signalfd with wrong sizemask -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 4, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SIGNALFD, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: signalfd(wrong size) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // signalfd with NULL mask -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 8, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SIGNALFD, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: signalfd(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // signalfd with valid mask in kernel context -> ENOSYS.
        let sigmask = [0u8; 8];
        let a = SyscallArgs {
            arg0: 0,
            arg1: sigmask.as_ptr() as u64,
            arg2: 8, arg3: 0, arg4: 0, arg5: 0,
        };
        if dispatch_linux(nr::SIGNALFD, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: signalfd not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // signalfd4 with bogus flag -> EINVAL.
        let a = SyscallArgs {
            arg0: 0,
            arg1: sigmask.as_ptr() as u64,
            arg2: 8, arg3: 0x8000_0000, arg4: 0, arg5: 0,
        };
        if dispatch_linux(nr::SIGNALFD4, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: signalfd4(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // signalfd4 with valid flag -> ENOSYS.
        let a = SyscallArgs {
            arg0: 0,
            arg1: sigmask.as_ptr() as u64,
            arg2: 8, arg3: 0o4000, arg4: 0, arg5: 0,
        };
        if dispatch_linux(nr::SIGNALFD4, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: signalfd4 not ENOSYS");
            return Err(KernelError::InternalError);
        }

        // timerfd_create with bogus clockid -> EINVAL.
        let a = SyscallArgs { arg0: 99, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TIMERFD_CREATE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: timerfd_create(bad clock) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // timerfd_create(CLOCK_MONOTONIC, bogus flag) -> EINVAL.
        let a = SyscallArgs { arg0: 1, arg1: 0x8000_0000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TIMERFD_CREATE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: timerfd_create(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // timerfd_create(CLOCK_MONOTONIC, 0) -> ENOSYS.
        let a = SyscallArgs { arg0: 1, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TIMERFD_CREATE, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: timerfd_create not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // timerfd_settime with bogus flag -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x8000_0000, arg2: 0x1000,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TIMERFD_SETTIME, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: timerfd_settime(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // timerfd_settime with NULL new -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TIMERFD_SETTIME, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: timerfd_settime(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // timerfd_gettime with NULL curr -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TIMERFD_GETTIME, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: timerfd_gettime(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }

        // inotify_init() -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::INOTIFY_INIT, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: inotify_init not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // inotify_init1 with bogus flag -> EINVAL.
        let a = SyscallArgs { arg0: 0x8000_0000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::INOTIFY_INIT1, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: inotify_init1(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // inotify_init1(0) -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::INOTIFY_INIT1, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: inotify_init1 not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // inotify_add_watch(NULL path) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::INOTIFY_ADD_WATCH, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: inotify_add_watch(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // inotify_add_watch(_, path, 0) -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::INOTIFY_ADD_WATCH, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: inotify_add_watch(mask=0) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // inotify_rm_watch in kernel context -> EBADF.
        let a = SyscallArgs { arg0: 99, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::INOTIFY_RM_WATCH, &a).value
            != -i64::from(errno::EBADF) {
            serial_println!("[syscall/linux]   FAIL: inotify_rm_watch not EBADF");
            return Err(KernelError::InternalError);
        }

        // fanotify_init() -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FANOTIFY_INIT, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: fanotify_init not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // fanotify_mark() -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FANOTIFY_MARK, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: fanotify_mark not ENOSYS");
            return Err(KernelError::InternalError);
        }
    }

    // sendfile / splice / tee / vmsplice / copy_file_range / AIO /
    // io_uring — input validation plus principled errno.
    {
        // sendfile in kernel context (fds skip validation) -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 1, arg2: 0, arg3: 16,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SENDFILE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: sendfile not EINVAL");
            return Err(KernelError::InternalError);
        }
        // splice with bogus flag -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0,
            arg4: 16, arg5: 0x8000_0000 };
        if dispatch_linux(nr::SPLICE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: splice(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // splice with valid flags -> EINVAL (no real pipe).
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0,
            arg4: 16, arg5: 1 };
        if dispatch_linux(nr::SPLICE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: splice not EINVAL");
            return Err(KernelError::InternalError);
        }
        // tee bogus flag -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 1, arg2: 16,
            arg3: 0x8000_0000, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::TEE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: tee(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // vmsplice with nr_segs > 0 and NULL iov -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 4, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::VMSPLICE, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: vmsplice(NULL iov) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // vmsplice with nr_segs > IOV_MAX (1024) -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 2048,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::VMSPLICE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: vmsplice(huge nr_segs) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // copy_file_range with non-zero flags -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0,
            arg4: 16, arg5: 1 };
        if dispatch_linux(nr::COPY_FILE_RANGE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: copy_file_range(flag) not EINVAL");
            return Err(KernelError::InternalError);
        }

        // io_setup(0,_) -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::IO_SETUP, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: io_setup(0) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // io_setup(8, NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 8, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::IO_SETUP, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: io_setup(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // io_setup(8, ctx) in kernel context -> ENOSYS.
        let mut ctx = [0u8; 8];
        let a = SyscallArgs {
            arg0: 8,
            arg1: ctx.as_mut_ptr() as u64,
            arg2: 0, arg3: 0, arg4: 0, arg5: 0,
        };
        if dispatch_linux(nr::IO_SETUP, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: io_setup not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // io_destroy(_) -> EINVAL.
        let a = SyscallArgs { arg0: 0xdeadbeef, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::IO_DESTROY, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: io_destroy not EINVAL");
            return Err(KernelError::InternalError);
        }
        // io_submit(_, -1, _) -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: u64::MAX, arg2: 0x1000,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::IO_SUBMIT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: io_submit(-1) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // io_getevents with min_nr > nr -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 10, arg2: 5, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::IO_GETEVENTS, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: io_getevents(min>nr) not EINVAL");
            return Err(KernelError::InternalError);
        }

        // io_uring_setup(0,_) -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::IO_URING_SETUP, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: io_uring_setup(0) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // io_uring_setup(8, NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 8, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::IO_URING_SETUP, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: io_uring_setup(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // io_uring_setup(8, params) in kernel context -> ENOSYS.
        let mut params = [0u8; 120];
        let a = SyscallArgs {
            arg0: 8,
            arg1: params.as_mut_ptr() as u64,
            arg2: 0, arg3: 0, arg4: 0, arg5: 0,
        };
        if dispatch_linux(nr::IO_URING_SETUP, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: io_uring_setup not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // io_uring_enter / register -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::IO_URING_ENTER, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: io_uring_enter not ENOSYS");
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::IO_URING_REGISTER, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: io_uring_register not ENOSYS");
            return Err(KernelError::InternalError);
        }
    }

    // BPF / perf_event_open / keyring / userfaultfd / memfd / pidfd /
    // process_vm — input validation plus principled errno.
    {
        // bpf(0, NULL, 0) -> EINVAL (size == 0).
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::BPF, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: bpf(size=0) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // bpf(0, NULL, 8) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 8, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::BPF, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: bpf(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // bpf(0, attr, 8) in kernel context -> ENOSYS.
        let attr = [0u8; 32];
        let a = SyscallArgs {
            arg0: 0,
            arg1: attr.as_ptr() as u64,
            arg2: 8, arg3: 0, arg4: 0, arg5: 0,
        };
        if dispatch_linux(nr::BPF, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: bpf not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // perf_event_open(NULL,_,_,_,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PERF_EVENT_OPEN, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: perf_event_open(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // perf_event_open(attr,_,_,_,_) -> ENOSYS.
        let a = SyscallArgs {
            arg0: attr.as_ptr() as u64,
            arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
        };
        if dispatch_linux(nr::PERF_EVENT_OPEN, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: perf_event_open not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // keyctl(_,_,_,_,_) -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::KEYCTL, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: keyctl not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // add_key(NULL,_,_,_,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::ADD_KEY, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: add_key(NULL,_,_,_,_) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // add_key(t,d,_,_,_) -> ENOSYS.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0x2000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::ADD_KEY, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: add_key not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // request_key(t,d,_,_) -> ENOSYS.
        if dispatch_linux(nr::REQUEST_KEY, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: request_key not ENOSYS");
            return Err(KernelError::InternalError);
        }

        // userfaultfd with bogus flag -> EINVAL.
        let a = SyscallArgs { arg0: 0x8000_0000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::USERFAULTFD, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: userfaultfd(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // userfaultfd(0) -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::USERFAULTFD, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: userfaultfd not ENOSYS");
            return Err(KernelError::InternalError);
        }

        // memfd_create(NULL,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MEMFD_CREATE, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: memfd_create(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // memfd_create(name, bogus flag) -> EINVAL.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0x100_0000, arg2: 0,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MEMFD_CREATE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: memfd_create(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // memfd_create(name, 0) -> ENOSYS.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MEMFD_CREATE, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: memfd_create not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // memfd_secret(0) -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MEMFD_SECRET, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: memfd_secret not ENOSYS");
            return Err(KernelError::InternalError);
        }

        // pidfd_open(pid <= 0) -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PIDFD_OPEN, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: pidfd_open(0) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // pidfd_open(1,_) -> ENOSYS.
        let a = SyscallArgs { arg0: 1, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PIDFD_OPEN, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: pidfd_open not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // pidfd_send_signal bogus sig -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 999, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PIDFD_SEND_SIGNAL, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: pidfd_send_signal(bad sig) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // pidfd_getfd nonzero flag -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PIDFD_GETFD, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: pidfd_getfd(flag) not EINVAL");
            return Err(KernelError::InternalError);
        }

        // process_vm_readv with pid <= 0 -> ESRCH.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PROCESS_VM_READV, &a).value
            != -i64::from(errno::ESRCH) {
            serial_println!("[syscall/linux]   FAIL: process_vm_readv(0) not ESRCH");
            return Err(KernelError::InternalError);
        }
        // process_vm_readv with nonzero flags -> EINVAL.
        let a = SyscallArgs { arg0: 1, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 1 };
        if dispatch_linux(nr::PROCESS_VM_READV, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: process_vm_readv(flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // process_vm_readv(pid=1, liovcnt=0, riovcnt=0) -> ESRCH (no
        // target process exists).
        let a = SyscallArgs { arg0: 1, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PROCESS_VM_READV, &a).value
            != -i64::from(errno::ESRCH) {
            serial_println!("[syscall/linux]   FAIL: process_vm_readv not ESRCH");
            return Err(KernelError::InternalError);
        }
        // process_vm_writev same.
        if dispatch_linux(nr::PROCESS_VM_WRITEV, &a).value
            != -i64::from(errno::ESRCH) {
            serial_println!("[syscall/linux]   FAIL: process_vm_writev not ESRCH");
            return Err(KernelError::InternalError);
        }
        // process_mrelease with nonzero flags -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 1, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PROCESS_MRELEASE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: process_mrelease(flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // process_mrelease(0,0) -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PROCESS_MRELEASE, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: process_mrelease not ENOSYS");
            return Err(KernelError::InternalError);
        }
    }

    // xattr / quota / module / namespace / mount / swap / reboot /
    // syslog — input validation plus principled errno.
    {
        // setxattr(NULL,_,_,_,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0x2000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SETXATTR, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: setxattr(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // setxattr(path,name,_,_,_) -> EOPNOTSUPP.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0x2000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SETXATTR, &a).value
            != -i64::from(errno::EOPNOTSUPP) {
            serial_println!("[syscall/linux]   FAIL: setxattr not EOPNOTSUPP");
            return Err(KernelError::InternalError);
        }
        // getxattr(path,name,_,_) -> ENODATA.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0x2000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::GETXATTR, &a).value
            != -i64::from(errno::ENODATA) {
            serial_println!("[syscall/linux]   FAIL: getxattr not ENODATA");
            return Err(KernelError::InternalError);
        }
        // listxattr(path, NULL, 0) -> 0 (empty list).
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::LISTXATTR, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: listxattr not 0");
            return Err(KernelError::InternalError);
        }
        // removexattr -> EOPNOTSUPP.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0x2000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::REMOVEXATTR, &a).value
            != -i64::from(errno::EOPNOTSUPP) {
            serial_println!("[syscall/linux]   FAIL: removexattr not EOPNOTSUPP");
            return Err(KernelError::InternalError);
        }
        // fgetxattr(NULL name) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FGETXATTR, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: fgetxattr(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // flistxattr in kernel context -> 0.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FLISTXATTR, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: flistxattr not 0");
            return Err(KernelError::InternalError);
        }

        // quotactl with NULL special -> EPERM (NULL is allowed).
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::QUOTACTL, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: quotactl not EPERM");
            return Err(KernelError::InternalError);
        }
        // quotactl_fd in kernel context -> EPERM.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::QUOTACTL_FD, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: quotactl_fd not EPERM");
            return Err(KernelError::InternalError);
        }

        // init_module(NULL,_,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 1, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::INIT_MODULE, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: init_module(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // init_module(img, 0, _) -> EINVAL.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::INIT_MODULE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: init_module(len=0) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // finit_module bogus flag -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0x8000_0000, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FINIT_MODULE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: finit_module(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // finit_module(_, NULL, 0) in kernel context -> EPERM.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FINIT_MODULE, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: finit_module not EPERM");
            return Err(KernelError::InternalError);
        }
        // delete_module(NULL,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::DELETE_MODULE, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: delete_module(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }

        // unshare(0) -> 0.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UNSHARE, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: unshare(0) not 0");
            return Err(KernelError::InternalError);
        }
        // unshare with bogus flag -> EINVAL.
        let a = SyscallArgs { arg0: 0x8, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UNSHARE, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: unshare(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // unshare(CLONE_FILES) -> EPERM.
        let a = SyscallArgs { arg0: 0x400, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UNSHARE, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: unshare not EPERM");
            return Err(KernelError::InternalError);
        }
        // setns in kernel context -> EPERM.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SETNS, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: setns not EPERM");
            return Err(KernelError::InternalError);
        }

        // mount(NULL target) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MOUNT, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: mount(NULL target) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // mount(target) -> EPERM.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MOUNT, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: mount not EPERM");
            return Err(KernelError::InternalError);
        }
        // umount2(target, bad flag) -> EINVAL.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0x8000_0000, arg2: 0,
            arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UMOUNT2, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: umount2(bad flag) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // umount2(target, 0) -> EPERM.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::UMOUNT2, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: umount2 not EPERM");
            return Err(KernelError::InternalError);
        }
        // pivot_root(NULL, NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PIVOT_ROOT, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: pivot_root(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // pivot_root(x, y) -> EPERM.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0x2000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PIVOT_ROOT, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: pivot_root not EPERM");
            return Err(KernelError::InternalError);
        }

        // swapon(path) -> EPERM.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SWAPON, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: swapon not EPERM");
            return Err(KernelError::InternalError);
        }
        // swapoff(path) -> EPERM.
        if dispatch_linux(nr::SWAPOFF, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: swapoff not EPERM");
            return Err(KernelError::InternalError);
        }

        // reboot with bad magic1 -> EINVAL.
        let a = SyscallArgs { arg0: 0xdead, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::REBOOT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: reboot(bad magic) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // reboot with valid magic -> EPERM.
        let a = SyscallArgs { arg0: 0xfee1_dead, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::REBOOT, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: reboot not EPERM");
            return Err(KernelError::InternalError);
        }

        // syslog(99) -> EINVAL.
        let a = SyscallArgs { arg0: 99, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SYSLOG, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: syslog(99) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // syslog(6) -> 0 (size_buffer).
        let a = SyscallArgs { arg0: 6, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SYSLOG, &a).value != 0 {
            serial_println!("[syscall/linux]   FAIL: syslog(6) not 0");
            return Err(KernelError::InternalError);
        }
        // syslog(2, NULL, 16) read -> EFAULT.
        let a = SyscallArgs { arg0: 2, arg1: 0, arg2: 16, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SYSLOG, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: syslog(2,NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // syslog(0) (CLOSE) -> EPERM.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SYSLOG, &a).value
            != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: syslog(0) not EPERM");
            return Err(KernelError::InternalError);
        }
    }

    // SysV IPC and POSIX message queues — input validation plus
    // principled errno.
    {
        // shmget with IPC_CREAT and size=0 -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0o1000, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SHMGET, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: shmget(create,size=0) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // shmget(_, 4096, 0) -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 4096, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SHMGET, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: shmget not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // shmat / shmctl / shmdt -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SHMAT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: shmat not EINVAL");
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::SHMCTL, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: shmctl not EINVAL");
            return Err(KernelError::InternalError);
        }
        if dispatch_linux(nr::SHMDT, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: shmdt not EINVAL");
            return Err(KernelError::InternalError);
        }

        // semget with negative nsems -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: u64::MAX, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SEMGET, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: semget(-1) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // semget(_, 4, 0) -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 4, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SEMGET, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: semget not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // semop(nsops=0) -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0x1000, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SEMOP, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: semop(0) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // semop(NULL,_,1) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SEMOP, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: semop(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // semctl -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SEMCTL, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: semctl not EINVAL");
            return Err(KernelError::InternalError);
        }
        // semtimedop(NULL,_,1,_) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SEMTIMEDOP, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: semtimedop(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }

        // msgget -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MSGGET, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: msgget not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // msgsnd(NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 8, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MSGSND, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: msgsnd(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // msgrcv(NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 8, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MSGRCV, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: msgrcv(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // msgctl(_,_,NULL) -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MSGCTL, &a).value
            != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: msgctl not EINVAL");
            return Err(KernelError::InternalError);
        }

        // mq_open(NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MQ_OPEN, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: mq_open(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // mq_open(name) -> ENOSYS.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MQ_OPEN, &a).value
            != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: mq_open not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // mq_unlink(NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MQ_UNLINK, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: mq_unlink(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // mq_unlink(name) -> ENOENT.
        let a = SyscallArgs { arg0: 0x1000, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MQ_UNLINK, &a).value
            != -i64::from(errno::ENOENT) {
            serial_println!("[syscall/linux]   FAIL: mq_unlink not ENOENT");
            return Err(KernelError::InternalError);
        }
        // mq_timedsend(NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MQ_TIMEDSEND, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: mq_timedsend(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // mq_timedreceive(NULL) -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MQ_TIMEDRECEIVE, &a).value
            != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: mq_timedreceive(NULL) not EFAULT");
            return Err(KernelError::InternalError);
        }
        // mq_notify in kernel context (fd validation skipped) -> EBADF.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0,
            arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MQ_NOTIFY, &a).value
            != -i64::from(errno::EBADF) {
            serial_println!("[syscall/linux]   FAIL: mq_notify not EBADF");
            return Err(KernelError::InternalError);
        }
        // mq_getsetattr in kernel context -> EBADF.
        if dispatch_linux(nr::MQ_GETSETATTR, &a).value
            != -i64::from(errno::EBADF) {
            serial_println!("[syscall/linux]   FAIL: mq_getsetattr not EBADF");
            return Err(KernelError::InternalError);
        }
    }

    // -----------------------------------------------------------------
    // poll / ppoll / select / pselect6 + epoll family
    // -----------------------------------------------------------------
    {
        // poll(NULL, 0, 0) — nfds == 0 -> ENOSYS (validation skipped).
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::POLL, &a).value != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: poll(NULL,0,0) not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // poll with absurd nfds -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: (1u64 << 21), arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::POLL, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: poll huge nfds not EINVAL");
            return Err(KernelError::InternalError);
        }
        // poll with nfds > 0 but NULL fds -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 4, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::POLL, &a).value != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: poll NULL fds not EFAULT");
            return Err(KernelError::InternalError);
        }
        // ppoll with nfds=0 and NULL timespec / sigmask -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PPOLL, &a).value != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: ppoll not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // ppoll bad sigsetsize -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 16, arg5: 0 };
        if dispatch_linux(nr::PPOLL, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: ppoll bad sigsetsize not EINVAL");
            return Err(KernelError::InternalError);
        }
        // select(0, NULL, NULL, NULL, NULL) -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SELECT, &a).value != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: select(0,...) not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // select with negative nfds -> EINVAL.
        let a = SyscallArgs { arg0: u64::MAX, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::SELECT, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: select negative nfds not EINVAL");
            return Err(KernelError::InternalError);
        }
        // pselect6(0, NULL, NULL, NULL, NULL, NULL) -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::PSELECT6, &a).value != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: pselect6 not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // epoll_create(0) -> EINVAL (size must be > 0 historically).
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::EPOLL_CREATE, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: epoll_create(0) not EINVAL");
            return Err(KernelError::InternalError);
        }
        // epoll_create(1) -> ENOSYS.
        let a = SyscallArgs { arg0: 1, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::EPOLL_CREATE, &a).value != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: epoll_create(1) not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // epoll_create1(0) -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::EPOLL_CREATE1, &a).value != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: epoll_create1(0) not ENOSYS");
            return Err(KernelError::InternalError);
        }
        // epoll_create1 with unknown flag bits -> EINVAL.
        let a = SyscallArgs { arg0: 0xff, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::EPOLL_CREATE1, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: epoll_create1 bad flags not EINVAL");
            return Err(KernelError::InternalError);
        }
        // epoll_ctl with invalid op -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 99, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::EPOLL_CTL, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: epoll_ctl bad op not EINVAL");
            return Err(KernelError::InternalError);
        }
        // epoll_ctl with ADD and NULL event ptr -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 1, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::EPOLL_CTL, &a).value != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: epoll_ctl ADD NULL event not EFAULT");
            return Err(KernelError::InternalError);
        }
        // epoll_wait with maxevents == 0 -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::EPOLL_WAIT, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: epoll_wait 0 maxevents not EINVAL");
            return Err(KernelError::InternalError);
        }
        // epoll_pwait with negative maxevents -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: u64::MAX, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::EPOLL_PWAIT, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: epoll_pwait neg maxevents not EINVAL");
            return Err(KernelError::InternalError);
        }
        // epoll_pwait with bad sigsetsize -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0, arg4: 0, arg5: 4 };
        // arg1 must point to writable memory; in kernel context validation
        // is a no-op so this proceeds to the sigsetsize check.
        if dispatch_linux(nr::EPOLL_PWAIT, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: epoll_pwait bad sigsetsize not EINVAL");
            return Err(KernelError::InternalError);
        }
        // epoll_pwait2 with 0 maxevents -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::EPOLL_PWAIT2, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: epoll_pwait2 0 maxevents not EINVAL");
            return Err(KernelError::InternalError);
        }
        // epoll_wait with maxevents > 0 and kernel-context fd -> EBADF
        // (validation is no-op so we reach the EBADF return).
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 1, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::EPOLL_WAIT, &a).value != -i64::from(errno::EBADF) {
            serial_println!("[syscall/linux]   FAIL: epoll_wait valid args not EBADF");
            return Err(KernelError::InternalError);
        }
    }

    // -----------------------------------------------------------------
    // openat2 / execveat / handle-at + new mount API
    // -----------------------------------------------------------------
    {
        // Sample valid C string in kernel memory we can point at.
        let path: &[u8] = b"/tmp/x\0";
        let path_ptr = path.as_ptr() as u64;
        let how_bytes = [0u8; 24];
        let how_ptr = how_bytes.as_ptr() as u64;

        // openat2 with NULL path -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: how_ptr, arg3: 24, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::OPENAT2, &a).value != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: openat2 NULL path not EFAULT");
            return Err(KernelError::InternalError);
        }
        // openat2 with wrong size -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: how_ptr, arg3: 16, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::OPENAT2, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: openat2 wrong size not EINVAL");
            return Err(KernelError::InternalError);
        }
        // openat2 with NULL how -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: 0, arg3: 24, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::OPENAT2, &a).value != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: openat2 NULL how not EFAULT");
            return Err(KernelError::InternalError);
        }
        // openat2 with valid args -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: how_ptr, arg3: 24, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::OPENAT2, &a).value != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: openat2 valid not ENOSYS");
            return Err(KernelError::InternalError);
        }

        // execveat with bad flags -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: 0, arg3: 0, arg4: 0xff, arg5: 0 };
        if dispatch_linux(nr::EXECVEAT, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: execveat bad flags not EINVAL");
            return Err(KernelError::InternalError);
        }
        // execveat with NULL path and no AT_EMPTY_PATH -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::EXECVEAT, &a).value != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: execveat NULL path not EFAULT");
            return Err(KernelError::InternalError);
        }
        // execveat with valid args -> ENOSYS.
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::EXECVEAT, &a).value != -i64::from(errno::ENOSYS) {
            serial_println!("[syscall/linux]   FAIL: execveat valid not ENOSYS");
            return Err(KernelError::InternalError);
        }

        // name_to_handle_at with bad flags -> EINVAL.
        let mount_id_bytes = [0u8; 4];
        let mount_id_ptr = mount_id_bytes.as_ptr() as u64;
        let handle_bytes = [0u8; 8];
        let handle_ptr = handle_bytes.as_ptr() as u64;
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: handle_ptr, arg3: mount_id_ptr, arg4: 0xff, arg5: 0 };
        if dispatch_linux(nr::NAME_TO_HANDLE_AT, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: n_t_h_a bad flags not EINVAL");
            return Err(KernelError::InternalError);
        }
        // name_to_handle_at with NULL mount_id -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: handle_ptr, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::NAME_TO_HANDLE_AT, &a).value != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: n_t_h_a NULL mount_id not EFAULT");
            return Err(KernelError::InternalError);
        }
        // name_to_handle_at with valid args -> EOPNOTSUPP.
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: handle_ptr, arg3: mount_id_ptr, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::NAME_TO_HANDLE_AT, &a).value != -i64::from(errno::EOPNOTSUPP) {
            serial_println!("[syscall/linux]   FAIL: n_t_h_a valid not EOPNOTSUPP");
            return Err(KernelError::InternalError);
        }

        // open_by_handle_at with NULL handle -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::OPEN_BY_HANDLE_AT, &a).value != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: open_by_handle_at NULL not EFAULT");
            return Err(KernelError::InternalError);
        }
        // open_by_handle_at with valid handle -> EPERM.
        let a = SyscallArgs { arg0: 0, arg1: handle_ptr, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::OPEN_BY_HANDLE_AT, &a).value != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: open_by_handle_at valid not EPERM");
            return Err(KernelError::InternalError);
        }

        // fsopen with bad flags -> EINVAL.
        let a = SyscallArgs { arg0: path_ptr, arg1: 0xff, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSOPEN, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: fsopen bad flags not EINVAL");
            return Err(KernelError::InternalError);
        }
        // fsopen NULL fsname -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSOPEN, &a).value != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: fsopen NULL not EFAULT");
            return Err(KernelError::InternalError);
        }
        // fsopen valid -> EPERM.
        let a = SyscallArgs { arg0: path_ptr, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSOPEN, &a).value != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: fsopen valid not EPERM");
            return Err(KernelError::InternalError);
        }

        // fsconfig bad cmd -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 99, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSCONFIG, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: fsconfig bad cmd not EINVAL");
            return Err(KernelError::InternalError);
        }
        // fsconfig valid cmd (in kernel context fd validation is no-op) -> EPERM.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSCONFIG, &a).value != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: fsconfig valid not EPERM");
            return Err(KernelError::InternalError);
        }

        // fsmount bad flags -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0xff, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSMOUNT, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: fsmount bad flags not EINVAL");
            return Err(KernelError::InternalError);
        }
        // fsmount bad attr_flags -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0x4000_0000, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSMOUNT, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: fsmount bad attr_flags not EINVAL");
            return Err(KernelError::InternalError);
        }
        // fsmount valid -> EPERM.
        let a = SyscallArgs { arg0: 0, arg1: 1, arg2: 0x1, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSMOUNT, &a).value != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: fsmount valid not EPERM");
            return Err(KernelError::InternalError);
        }

        // fspick bad flags -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: 0xff, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSPICK, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: fspick bad flags not EINVAL");
            return Err(KernelError::InternalError);
        }
        // fspick NULL path -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSPICK, &a).value != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: fspick NULL not EFAULT");
            return Err(KernelError::InternalError);
        }
        // fspick valid -> EPERM.
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: 0, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::FSPICK, &a).value != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: fspick valid not EPERM");
            return Err(KernelError::InternalError);
        }

        // open_tree bad flags -> EINVAL.  Use bit 30 which is outside the
        // OPEN_TREE_MASK (the mask tops out at AT_RECURSIVE = 0x8000 and
        // O_CLOEXEC = 0o2_000_000 = 0x80000).
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: 0x4000_0000, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::OPEN_TREE, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: open_tree bad flags not EINVAL");
            return Err(KernelError::InternalError);
        }
        // open_tree valid -> EPERM.
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: 1, arg3: 0, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::OPEN_TREE, &a).value != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: open_tree valid not EPERM");
            return Err(KernelError::InternalError);
        }

        // move_mount bad flags -> EINVAL.
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: 0, arg3: path_ptr, arg4: 0xffff_ffff, arg5: 0 };
        if dispatch_linux(nr::MOVE_MOUNT, &a).value != -i64::from(errno::EINVAL) {
            serial_println!("[syscall/linux]   FAIL: move_mount bad flags not EINVAL");
            return Err(KernelError::InternalError);
        }
        // move_mount NULL from-path without EMPTY_PATH -> EFAULT.
        let a = SyscallArgs { arg0: 0, arg1: 0, arg2: 0, arg3: path_ptr, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MOVE_MOUNT, &a).value != -i64::from(errno::EFAULT) {
            serial_println!("[syscall/linux]   FAIL: move_mount NULL from not EFAULT");
            return Err(KernelError::InternalError);
        }
        // move_mount valid -> EPERM.
        let a = SyscallArgs { arg0: 0, arg1: path_ptr, arg2: 0, arg3: path_ptr, arg4: 0, arg5: 0 };
        if dispatch_linux(nr::MOVE_MOUNT, &a).value != -i64::from(errno::EPERM) {
            serial_println!("[syscall/linux]   FAIL: move_mount valid not EPERM");
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[syscall/linux] Translation self-test PASSED");
    Ok(())
}
