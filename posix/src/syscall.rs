//! Raw syscall primitives.
//!
//! Provides inline assembly wrappers for issuing our native syscalls
//! from userspace via the x86_64 SYSCALL instruction.
//!
//! ## ABI
//!
//! ```text
//! RAX = syscall number
//! RDI = arg0, RSI = arg1, RDX = arg2, R10 = arg3, R8 = arg4, R9 = arg5
//! Return: RAX (negative = error code)
//! ```
//!
//! This matches the Linux x86_64 syscall convention.

// ---------------------------------------------------------------------------
// Native syscall numbers (must match kernel/src/syscall/number.rs)
// ---------------------------------------------------------------------------

pub const SYS_EXIT: u64 = 1;
pub const SYS_TASK_ID: u64 = 2;
// Process ID lives in the kernel-process zone (500–599), not kernel-core.
// (Previously mis-numbered as 3, which the kernel does not implement —
// getpid() was hitting an unimplemented syscall.  See number.rs.)
pub const SYS_PROCESS_ID: u64 = 502;
pub const SYS_CLOCK_MONOTONIC: u64 = 10;
pub const SYS_CLOCK_REALTIME: u64 = 14;
pub const SYS_CLOCK_SETTIME: u64 = 15;
pub const SYS_CLOCK_ADJTIME: u64 = 16;
pub const SYS_SLEEP: u64 = 11;

// Console I/O
pub const SYS_CONSOLE_WRITE: u64 = 100;
pub const SYS_CONSOLE_READ_CHAR: u64 = 101;

// Kernel log ring buffer (read-only).
//   READ: (after_seq, buf_ptr, buf_cap) -> (entry_count in value,
//         newest_seq in value2).  Pass `u64::MAX` as after_seq to read
//         from the oldest available entry; otherwise reads entries with
//         seq > after_seq.  Fills the buffer with JSON-lines text (one
//         JSON object per line, each terminated with `\n`).  Non-consuming.
pub const SYS_LOG_READ: u64 = 102;

// Memory management.
//
// These MUST match the kernel's *native* syscall table
// (kernel/src/syscall/number.rs), NOT the Linux-ABI numbers.  They were
// previously mis-numbered 30/31/32, which collide with the kernel's IRQ
// syscalls (SYS_IRQ_REGISTER=30, SYS_IRQ_WAIT=31, SYS_IRQ_RELEASE=32) —
// so a native mmap() actually hit the capability-gated IRQ register path
// and came back PermissionDenied (-400).  That silently broke the crt's
// main-thread TLS setup on native binaries (the fastpy/initiative-F path).
pub const SYS_MMAP: u64 = 20;
pub const SYS_MUNMAP: u64 = 21;
pub const SYS_MPROTECT: u64 = 22;

// Scheduler / thread
pub const SYS_SCHED_SET_PROFILE: u64 = 53;
pub const SYS_CPU_COUNT: u64 = 55;
pub const SYS_PHYS_PAGES_TOTAL: u64 = 56;
pub const SYS_PHYS_PAGES_AVAIL: u64 = 57;
pub const SYS_LOADAVG: u64 = 58;
pub const SYS_CPU_TIMES: u64 = 59;

// Process management
pub const SYS_PROCESS_SPAWN: u64 = 500;
pub const SYS_PROCESS_WAIT: u64 = 501;
pub const SYS_PROCESS_EXEC: u64 = 503;
pub const SYS_PROCESS_TRY_WAIT: u64 = 507;
pub const SYS_PROCESS_IS_READY: u64 = 509;
pub const SYS_THREAD_CREATE: u64 = 510;
pub const SYS_THREAD_EXIT: u64 = 511;
pub const SYS_THREAD_JOIN: u64 = 512;
pub const SYS_PROCESS_KILL: u64 = 506;
pub const SYS_PROCESS_SPAWN_EX: u64 = 517;
pub const SYS_PROCESS_GET_INITIAL_FDS: u64 = 518;
pub const SYS_PROCESS_GET_ARGS: u64 = 519;
pub const SYS_PROCESS_PARENT_ID: u64 = 520;
pub const SYS_PROCESS_COUNT: u64 = 521;

// POSIX signal shim (522–526)
pub const SYS_SIGNAL_REGISTER: u64 = 522;
pub const SYS_SIGNAL_SEND: u64 = 523;
pub const SYS_SIGNAL_RETURN: u64 = 524;
pub const SYS_SIGNAL_MASK: u64 = 525;
pub const SYS_SIGNAL_PENDING: u64 = 526;

/// Fork the calling process (copy-on-write).  Returns the child PID to
/// the parent and 0 to the child, or a negative error code to the
/// parent on failure.
pub const SYS_PROCESS_FORK: u64 = 527;

/// Set the calling thread's `fs_base` (the x86-64 thread pointer / TLS
/// base).  `arg0` is the new base address, which must be < 2^47.  The
/// kernel writes `IA32_FS_BASE` and persists the value on the task so it
/// survives context switches.  Native counterpart of Linux
/// `arch_prctl(ARCH_SET_FS, addr)`.  Used by the crt to install
/// main-thread ELF TLS on a native (aux-vector-less) static binary.
///
/// Returns: 0 on success, or a negative error code (InvalidArgument if
/// the address is out of range).
pub const SYS_SET_FS_BASE: u64 = 528;

// Filesystem
pub const SYS_FS_READ_FILE: u64 = 600;
pub const SYS_FS_WRITE_FILE: u64 = 601;
pub const SYS_FS_DELETE: u64 = 602;
pub const SYS_FS_LIST_DIR: u64 = 603;
pub const SYS_FS_MKDIR: u64 = 604;
pub const SYS_FS_RMDIR: u64 = 605;
pub const SYS_FS_STAT: u64 = 606;
pub const SYS_FS_LINK: u64 = 607;
pub const SYS_FS_STATVFS: u64 = 608;
// Advisory whole-file locks (flock).  FLOCK args: (path_ptr, path_len,
// lock_type, owner) where lock_type 0=shared, 1=exclusive and owner is
// the lock-holder ID (we use the process ID).  Non-blocking: returns
// WouldBlock on contention.  FUNLOCK args: (path_ptr, path_len, owner).
pub const SYS_FS_FLOCK: u64 = 609;
pub const SYS_FS_FUNLOCK: u64 = 640;
pub const SYS_FS_OPEN: u64 = 610;
pub const SYS_FS_CLOSE: u64 = 611;
pub const SYS_FS_READ: u64 = 612;
pub const SYS_FS_WRITE: u64 = 613;
pub const SYS_FS_SEEK: u64 = 614;
// Sparse-file seek: find the next data region (SEEK_DATA) or hole
// (SEEK_HOLE) at or after a byte offset.  Args: (handle, offset) ->
// resulting position.  Used by lseek's SEEK_DATA/SEEK_HOLE whence values.
pub const SYS_FS_SEEK_DATA: u64 = 650;
pub const SYS_FS_SEEK_HOLE: u64 = 651;
pub const SYS_FS_TRUNCATE: u64 = 615;
pub const SYS_FS_RENAME: u64 = 616;
pub const SYS_FS_FSTAT: u64 = 617;
pub const SYS_FS_DUP: u64 = 645;
pub const SYS_FS_COPY: u64 = 642;
pub const SYS_FS_APPEND: u64 = 643;
pub const SYS_FS_FTRUNCATE: u64 = 644;

// Symlinks
pub const SYS_FS_SYMLINK: u64 = 637;
pub const SYS_FS_READLINK: u64 = 638;
pub const SYS_FS_LSTAT: u64 = 639;

// Timestamps: set (a)ccess/(m)odify times.  Args: (path_ptr, path_len,
// accessed_ns, modified_ns) where 0 means "leave this timestamp unchanged".
pub const SYS_FS_SET_TIMES: u64 = 632;

// Ownership: set uid/gid.  Args: (path_ptr, path_len, uid, gid) where
// u32::MAX in a field means "leave that field unchanged" (POSIX chown).
pub const SYS_FS_SET_OWNER: u64 = 630;

// Permissions: set Unix mode bits.  Args: (path_ptr, path_len, perms) where
// perms is masked to the low 0o7777 bits by the kernel.
pub const SYS_FS_SET_PERMS: u64 = 631;

// Extended attributes.
//   GET:    (path_ptr, path_len, key_ptr, val_ptr, val_cap) -> true value
//           length (val_cap 0 = size query; copies min(len, cap) bytes).
//   SET:    (path_ptr, path_len, key_ptr, val_ptr, val_len) -> 0.
//   REMOVE: (path_ptr, path_len, key_ptr) -> 0.
//   LIST:   (path_ptr, path_len, buf_ptr, buf_cap) -> total bytes of the
//           null-terminated key list (buf_cap 0 = size query; only fills
//           when the whole list fits).
pub const SYS_FS_GET_XATTR: u64 = 633;
pub const SYS_FS_SET_XATTR: u64 = 634;
pub const SYS_FS_REMOVE_XATTR: u64 = 635;
pub const SYS_FS_LIST_XATTRS: u64 = 636;

// Sync
pub const SYS_FS_SYNC: u64 = 641;

// Filesystem change notification (inotify backend).
//   CREATE: (path_ptr, path_len, event_mask, flags) -> watch id.
//           event_mask bits: 0=CREATE 1=DELETE 2=MODIFY 3=RENAME
//           4=METADATA 5=ACCESS; flags bit0 = recursive.
//   READ:   (watch_id, buf_ptr, max_events) -> event count.  Each event
//           is FS_WATCH_EVENT_SIZE bytes: [0..256] affected path,
//           [256..512] new path (rename), [512..520] watch id (u64),
//           [520..524] event type (u32: 0=created 1=deleted 2=modified
//           3=renamed 4=metadata 5=accessed 255=overflow), [524..528] pad.
//   CLOSE:  (watch_id) -> 0.
pub const SYS_FS_WATCH_CREATE: u64 = 622;
pub const SYS_FS_WATCH_READ: u64 = 623;
pub const SYS_FS_WATCH_CLOSE: u64 = 624;

/// Size in bytes of one event record returned by `SYS_FS_WATCH_READ`.
pub const FS_WATCH_EVENT_SIZE: usize = 528;

// Pipes (IPC range 200-399)
pub const SYS_PIPE_CREATE: u64 = 220;
pub const SYS_PIPE_WRITE: u64 = 221;
pub const SYS_PIPE_READ: u64 = 222;
pub const SYS_PIPE_TRY_WRITE: u64 = 223;
pub const SYS_PIPE_TRY_READ: u64 = 224;
pub const SYS_PIPE_CLOSE: u64 = 225;
pub const SYS_PIPE_POLL: u64 = 228;
pub const SYS_PIPE_READABLE_BYTES: u64 = 229;
// Later pipe additions live in the free extension range (657+): the original
// 220-229 block is full (230 starts shared memory). Backs tee(2) — peek copies
// buffered bytes without consuming, wait_readable blocks for data/EOF.
pub const SYS_PIPE_PEEK: u64 = 657;
pub const SYS_PIPE_WAIT_READABLE: u64 = 658;

// Stream sockets (IPC range 300-310) — bidirectional byte streams backing
// socketpair(AF_UNIX, SOCK_STREAM, ...).  Mirrors kernel/src/syscall/number.rs.
pub const SYS_SOCKETPAIR_CREATE: u64 = 300;
pub const SYS_SOCKETPAIR_SEND: u64 = 301;
pub const SYS_SOCKETPAIR_RECV: u64 = 302;
pub const SYS_SOCKETPAIR_TRY_SEND: u64 = 303;
pub const SYS_SOCKETPAIR_TRY_RECV: u64 = 304;
pub const SYS_SOCKETPAIR_CLOSE: u64 = 305;
pub const SYS_SOCKETPAIR_SEND_TIMEOUT: u64 = 306;
pub const SYS_SOCKETPAIR_RECV_TIMEOUT: u64 = 307;
pub const SYS_SOCKETPAIR_POLL: u64 = 308;
pub const SYS_SOCKETPAIR_READABLE_BYTES: u64 = 309;
pub const SYS_SOCKETPAIR_SHUTDOWN: u64 = 310;

// Futexes (IPC range 210-214)
pub const SYS_FUTEX_WAIT: u64 = 210;
pub const SYS_FUTEX_WAKE: u64 = 211;
pub const SYS_FUTEX_LOCK_PI: u64 = 212;
pub const SYS_FUTEX_UNLOCK_PI: u64 = 213;
pub const SYS_FUTEX_WAIT_TIMEOUT: u64 = 214;

// Eventfd (IPC range 240-249)
pub const SYS_EVENTFD_CREATE: u64 = 240;
pub const SYS_EVENTFD_WRITE: u64 = 241;
pub const SYS_EVENTFD_READ: u64 = 242;
pub const SYS_EVENTFD_TRY_READ: u64 = 243;
pub const SYS_EVENTFD_CLOSE: u64 = 244;
pub const SYS_EVENTFD_READ_TIMEOUT: u64 = 245;
pub const SYS_EVENTFD_WRITE_TIMEOUT: u64 = 246;
pub const SYS_EVENTFD_HAS_VALUE: u64 = 247;

// Networking (800-999)
pub const SYS_TCP_CONNECT: u64 = 800;
pub const SYS_TCP_SEND: u64 = 801;
pub const SYS_TCP_RECV: u64 = 802;
pub const SYS_TCP_CLOSE: u64 = 803;
pub const SYS_TCP_BIND: u64 = 804;
pub const SYS_TCP_ACCEPT: u64 = 805;
pub const SYS_TCP_CLOSE_LISTENER: u64 = 806;
pub const SYS_TCP_ABORT: u64 = 807;
pub const SYS_TCP_PEER_ADDR: u64 = 808;

pub const SYS_UDP_BIND: u64 = 810;
pub const SYS_UDP_SEND: u64 = 811;
pub const SYS_UDP_RECV: u64 = 812;
pub const SYS_UDP_CLOSE: u64 = 813;
pub const SYS_UDP_MCAST_JOIN: u64 = 814;
pub const SYS_UDP_MCAST_LEAVE: u64 = 815;
pub const SYS_UDP_CONNECT: u64 = 816;
pub const SYS_UDP_LOCAL_PORT: u64 = 817;

pub const SYS_DNS_RESOLVE: u64 = 820;
pub const SYS_DNS_REVERSE_RESOLVE: u64 = 821;
pub const SYS_NET_STAT: u64 = 825;
pub const SYS_ICMP_PING: u64 = 830;
pub const SYS_ICMP_PING_WAIT: u64 = 831;
pub const SYS_TCP_LIST: u64 = 840;
pub const SYS_TCP_LISTENER_LIST: u64 = 841;
pub const SYS_NET_IF_INFO: u64 = 842;
pub const SYS_ARP_TABLE: u64 = 843;
pub const SYS_DNS_CACHE_STATS: u64 = 844;
pub const SYS_TCP_POLL_STATUS: u64 = 845;
pub const SYS_TCP_LISTENER_READY: u64 = 846;
pub const SYS_UDP_RX_READY: u64 = 847;
pub const SYS_UDP_RX_FRONT_BYTES: u64 = 848;
pub const SYS_TCP_SHUTDOWN: u64 = 855;
pub const SYS_TCP_INFO: u64 = 849;
pub const SYS_TCP_SET_NODELAY: u64 = 850;
pub const SYS_TCP_SET_KEEPALIVE: u64 = 851;
pub const SYS_TCP_SET_KEEPALIVE_PARAMS: u64 = 852;
pub const SYS_TCP_LAST_ERROR: u64 = 853;
pub const SYS_TCP_LOCAL_PORT: u64 = 854;

// ---------------------------------------------------------------------------
// Inline syscall wrappers
// ---------------------------------------------------------------------------
//
// Host-build safety gate
// ----------------------
// `syscallN()` issues a raw `SYSCALL` x86_64 instruction.  On our OS
// target (`target_os = "none"`, the bare-metal posix staticlib) that
// instruction transfers control to the kernel's syscall entry.  On any
// host build (`not(target_os = "none")`, used by `cargo test` against
// the host triple) the same instruction transfers control to whatever
// the host OS placed at SYSCALL — on Windows it dispatches to NT
// system services, with completely different ABI and semantics.
//
// To prevent that UB during host test runs we gate the inline asm
// behind `cfg(target_os = "none")` and have host builds return a
// documented sentinel (`-ENOSYS`).  Wrapper functions that need
// host-meaningful behaviour (e.g. `getpid`, `eventfd`, `timerfd_create`)
// detect this sentinel via `errno::translate` and either fall back to
// a host-friendly implementation or fail cleanly.  Tests that need to
// exercise post-syscall validator logic on host use the dedicated
// test-only fdtable helpers (see `fdtable::test_install_handle_kind`)
// rather than calling the real wrappers.

/// Sentinel returned by every `syscallN()` on host builds.  Equals
/// `-(errno::ENOSYS as i64)`.  Pinned by `host_enosys_matches_errno_module`
/// so a future renumbering of ENOSYS won't drift this value.
#[cfg(not(target_os = "none"))]
const HOST_ENOSYS: i64 = -38;

// Host-only shim for the wall-clock / monotonic-clock syscalls.  These
// are by far the most-called bare `syscall0` in the posix crate (>20
// call sites in epoll/poll/socket/file/time/sys_times/unistd), and on
// the host build the raw SYSCALL is gated off so they would otherwise
// all return `HOST_ENOSYS` and silently break time-dependent tests.
//
// We back them with `std::time` here so any wrapper that calls
// `clock_gettime` / reads SYS_CLOCK_MONOTONIC for a timeout deadline /
// stamps a record with the realtime clock just works on host — no
// per-call-site intercepts needed.  Same pattern as `host_eventfd_sim`
// in `epoll.rs`.
#[cfg(not(target_os = "none"))]
mod host_clock {
    extern crate std;
    use std::sync::OnceLock;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    /// First call captures the "boot" instant; subsequent calls return
    /// nanoseconds since then.  Monotonic, non-decreasing, no wall-clock
    /// dependency — matches `SYS_CLOCK_MONOTONIC`'s contract.
    static BOOT: OnceLock<Instant> = OnceLock::new();

    pub fn monotonic_ns() -> i64 {
        let boot = *BOOT.get_or_init(Instant::now);
        let ns = Instant::now().saturating_duration_since(boot).as_nanos();
        // Saturate to i64::MAX (~292 years from boot) rather than wrap.
        i64::try_from(ns).unwrap_or(i64::MAX)
    }

    pub fn realtime_ns() -> i64 {
        // `SystemTime` can predate UNIX_EPOCH on systems with broken
        // clocks; clamp to 0 in that case (matches what an uninitialised
        // RTC would return on the OS target).
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
            Err(_) => 0,
        }
    }
}

/// Issue a syscall with 0 arguments.
#[inline(always)]
#[must_use]
pub fn syscall0(nr: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: The SYSCALL instruction is the defined kernel entry
        // point on our OS target.  RCX and R11 are clobbered by SYSCALL
        // (saves RIP and RFLAGS).
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        // Host-side intercepts: the clock syscalls are routed to
        // std::time so time-dependent code paths work in unit tests.
        // Everything else returns the ENOSYS sentinel.
        match nr {
            SYS_CLOCK_MONOTONIC => host_clock::monotonic_ns(),
            SYS_CLOCK_REALTIME => host_clock::realtime_ns(),
            _ => HOST_ENOSYS,
        }
    }
}

/// Issue a syscall with 1 argument.
#[inline(always)]
#[must_use]
pub fn syscall1(nr: u64, arg0: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0);
        HOST_ENOSYS
    }
}

/// Issue a syscall with 2 arguments.
#[inline(always)]
#[must_use]
pub fn syscall2(nr: u64, arg0: u64, arg1: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                in("rsi") arg1,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0, arg1);
        HOST_ENOSYS
    }
}

/// Issue a syscall with 3 arguments.
#[inline(always)]
#[must_use]
pub fn syscall3(nr: u64, arg0: u64, arg1: u64, arg2: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                in("rsi") arg1,
                in("rdx") arg2,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0, arg1, arg2);
        HOST_ENOSYS
    }
}

/// Issue a syscall with 3 arguments, capturing both return values.
///
/// Returns `(value, value2)` = `(rax, rdx)`.  Used for syscalls that
/// reply with `SyscallResult::ok2` (two-value returns), e.g.
/// `SYS_LOG_READ` returns `(entry_count, newest_seq)`.
#[inline(always)]
#[must_use]
pub fn syscall3_2ret(nr: u64, arg0: u64, arg1: u64, arg2: u64) -> (i64, i64) {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        let ret2: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.  RAX holds `value`,
        // RDX holds `value2` on return.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                in("rsi") arg1,
                in("rdx") arg2,
                lateout("rax") ret,
                lateout("rdx") ret2,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        (ret, ret2)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0, arg1, arg2);
        (HOST_ENOSYS, 0)
    }
}

/// Issue a syscall with 4 arguments.
#[inline(always)]
#[must_use]
pub fn syscall4(nr: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                in("rsi") arg1,
                in("rdx") arg2,
                in("r10") arg3,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0, arg1, arg2, arg3);
        HOST_ENOSYS
    }
}

/// Issue a syscall with 5 arguments.
#[inline(always)]
#[must_use]
pub fn syscall5(nr: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                in("rsi") arg1,
                in("rdx") arg2,
                in("r10") arg3,
                in("r8") arg4,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0, arg1, arg2, arg3, arg4);
        HOST_ENOSYS
    }
}

/// Issue a syscall with 6 arguments.
#[inline(always)]
#[must_use]
pub fn syscall6(nr: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                in("rsi") arg1,
                in("rdx") arg2,
                in("r10") arg3,
                in("r8") arg4,
                in("r9") arg5,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0, arg1, arg2, arg3, arg4, arg5);
        HOST_ENOSYS
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Syscall numbers are non-zero --

    #[test]
    fn syscall_numbers_nonzero() {
        // Syscall number 0 is reserved (invalid).
        let all_numbers = [
            SYS_EXIT,
            SYS_TASK_ID,
            SYS_PROCESS_ID,
            SYS_CLOCK_MONOTONIC,
            SYS_SLEEP,
            SYS_CONSOLE_WRITE,
            SYS_CONSOLE_READ_CHAR,
            SYS_LOG_READ,
            SYS_MMAP,
            SYS_MUNMAP,
            SYS_MPROTECT,
            SYS_SCHED_SET_PROFILE,
            SYS_PROCESS_SPAWN,
            SYS_PROCESS_WAIT,
            SYS_PROCESS_EXEC,
            SYS_PROCESS_TRY_WAIT,
            SYS_PROCESS_FORK,
            SYS_THREAD_CREATE,
            SYS_THREAD_EXIT,
            SYS_THREAD_JOIN,
            SYS_PROCESS_SPAWN_EX,
            SYS_PROCESS_GET_INITIAL_FDS,
            SYS_PROCESS_GET_ARGS,
            SYS_FS_READ_FILE,
            SYS_FS_WRITE_FILE,
            SYS_FS_DELETE,
            SYS_FS_LIST_DIR,
            SYS_FS_MKDIR,
            SYS_FS_RMDIR,
            SYS_FS_STAT,
            SYS_FS_LINK,
            SYS_FS_STATVFS,
            SYS_FS_OPEN,
            SYS_FS_CLOSE,
            SYS_FS_READ,
            SYS_FS_WRITE,
            SYS_FS_SEEK,
            SYS_FS_TRUNCATE,
            SYS_FS_RENAME,
            SYS_FS_FSTAT,
            SYS_FS_DUP,
            SYS_FS_COPY,
            SYS_FS_APPEND,
            SYS_FS_FTRUNCATE,
            SYS_FS_SYMLINK,
            SYS_FS_READLINK,
            SYS_FS_LSTAT,
            SYS_FS_SYNC,
            SYS_FS_FLOCK,
            SYS_FS_FUNLOCK,
            SYS_FS_SEEK_DATA,
            SYS_FS_SEEK_HOLE,
            SYS_FS_WATCH_CREATE,
            SYS_FS_WATCH_READ,
            SYS_FS_WATCH_CLOSE,
            SYS_FS_SET_TIMES,
            SYS_FS_SET_OWNER,
            SYS_FS_SET_PERMS,
            SYS_FS_GET_XATTR,
            SYS_FS_SET_XATTR,
            SYS_FS_REMOVE_XATTR,
            SYS_FS_LIST_XATTRS,
            SYS_PIPE_CREATE,
            SYS_PIPE_WRITE,
            SYS_PIPE_READ,
            SYS_PIPE_TRY_WRITE,
            SYS_PIPE_TRY_READ,
            SYS_PIPE_CLOSE,
            SYS_PIPE_POLL,
            SYS_PIPE_READABLE_BYTES,
            SYS_FUTEX_WAIT,
            SYS_FUTEX_WAKE,
            SYS_FUTEX_LOCK_PI,
            SYS_FUTEX_UNLOCK_PI,
            SYS_FUTEX_WAIT_TIMEOUT,
            SYS_EVENTFD_CREATE,
            SYS_EVENTFD_WRITE,
            SYS_EVENTFD_READ,
            SYS_EVENTFD_TRY_READ,
            SYS_EVENTFD_CLOSE,
            SYS_EVENTFD_READ_TIMEOUT,
            SYS_EVENTFD_WRITE_TIMEOUT,
            SYS_EVENTFD_HAS_VALUE,
            SYS_TCP_CONNECT,
            SYS_TCP_SEND,
            SYS_TCP_RECV,
            SYS_TCP_CLOSE,
            SYS_TCP_BIND,
            SYS_TCP_ACCEPT,
            SYS_TCP_CLOSE_LISTENER,
            SYS_TCP_ABORT,
            SYS_TCP_PEER_ADDR,
            SYS_UDP_BIND,
            SYS_UDP_SEND,
            SYS_UDP_RECV,
            SYS_UDP_CLOSE,
            SYS_UDP_MCAST_JOIN,
            SYS_UDP_MCAST_LEAVE,
            SYS_UDP_CONNECT,
            SYS_UDP_LOCAL_PORT,
            SYS_DNS_RESOLVE,
            SYS_DNS_REVERSE_RESOLVE,
            SYS_NET_STAT,
            SYS_ICMP_PING,
            SYS_ICMP_PING_WAIT,
            SYS_TCP_LIST,
            SYS_TCP_LISTENER_LIST,
            SYS_NET_IF_INFO,
            SYS_ARP_TABLE,
            SYS_DNS_CACHE_STATS,
            SYS_TCP_POLL_STATUS,
            SYS_TCP_LISTENER_READY,
            SYS_UDP_RX_READY,
            SYS_UDP_RX_FRONT_BYTES,
            SYS_TCP_SHUTDOWN,
            SYS_TCP_INFO,
            SYS_TCP_SET_NODELAY,
            SYS_TCP_SET_KEEPALIVE,
            SYS_TCP_SET_KEEPALIVE_PARAMS,
            SYS_TCP_LAST_ERROR,
            SYS_TCP_LOCAL_PORT,
        ];
        for &nr in &all_numbers {
            assert_ne!(nr, 0, "syscall number must not be zero");
        }
    }

    // -- Process-control syscall numbers match the kernel ABI --

    #[test]
    fn process_syscall_numbers_match_kernel() {
        // These must equal the values in kernel/src/syscall/number.rs.
        // A mismatch silently routes a POSIX call to the wrong (or an
        // unimplemented) kernel syscall.
        assert_eq!(SYS_EXIT, 1);
        assert_eq!(SYS_TASK_ID, 2);
        assert_eq!(SYS_PROCESS_ID, 502, "getpid ABI number drifted");
        assert_eq!(SYS_PROCESS_FORK, 527, "fork ABI number drifted");
        assert_eq!(SYS_PROCESS_SPAWN, 500);
        assert_eq!(SYS_PROCESS_EXEC, 503);
        assert_eq!(SYS_PROCESS_PARENT_ID, 520);
    }

    // -- All syscall numbers are unique --

    #[test]
    fn syscall_numbers_unique() {
        let all_numbers: &[u64] = &[
            SYS_EXIT,
            SYS_TASK_ID,
            SYS_PROCESS_ID,
            SYS_CLOCK_MONOTONIC,
            SYS_SLEEP,
            SYS_CONSOLE_WRITE,
            SYS_CONSOLE_READ_CHAR,
            SYS_LOG_READ,
            SYS_MMAP,
            SYS_MUNMAP,
            SYS_MPROTECT,
            SYS_SCHED_SET_PROFILE,
            SYS_PROCESS_SPAWN,
            SYS_PROCESS_WAIT,
            SYS_PROCESS_EXEC,
            SYS_PROCESS_TRY_WAIT,
            SYS_PROCESS_FORK,
            SYS_THREAD_CREATE,
            SYS_THREAD_EXIT,
            SYS_THREAD_JOIN,
            SYS_PROCESS_SPAWN_EX,
            SYS_PROCESS_GET_INITIAL_FDS,
            SYS_PROCESS_GET_ARGS,
            SYS_FS_READ_FILE,
            SYS_FS_WRITE_FILE,
            SYS_FS_DELETE,
            SYS_FS_LIST_DIR,
            SYS_FS_MKDIR,
            SYS_FS_RMDIR,
            SYS_FS_STAT,
            SYS_FS_LINK,
            SYS_FS_STATVFS,
            SYS_FS_OPEN,
            SYS_FS_CLOSE,
            SYS_FS_READ,
            SYS_FS_WRITE,
            SYS_FS_SEEK,
            SYS_FS_TRUNCATE,
            SYS_FS_RENAME,
            SYS_FS_FSTAT,
            SYS_FS_DUP,
            SYS_FS_COPY,
            SYS_FS_APPEND,
            SYS_FS_FTRUNCATE,
            SYS_FS_SYMLINK,
            SYS_FS_READLINK,
            SYS_FS_LSTAT,
            SYS_FS_SYNC,
            SYS_FS_FLOCK,
            SYS_FS_FUNLOCK,
            SYS_FS_SEEK_DATA,
            SYS_FS_SEEK_HOLE,
            SYS_FS_WATCH_CREATE,
            SYS_FS_WATCH_READ,
            SYS_FS_WATCH_CLOSE,
            SYS_FS_SET_TIMES,
            SYS_FS_SET_OWNER,
            SYS_FS_SET_PERMS,
            SYS_FS_GET_XATTR,
            SYS_FS_SET_XATTR,
            SYS_FS_REMOVE_XATTR,
            SYS_FS_LIST_XATTRS,
            SYS_PIPE_CREATE,
            SYS_PIPE_WRITE,
            SYS_PIPE_READ,
            SYS_PIPE_TRY_WRITE,
            SYS_PIPE_TRY_READ,
            SYS_PIPE_CLOSE,
            SYS_PIPE_POLL,
            SYS_PIPE_READABLE_BYTES,
            SYS_FUTEX_WAIT,
            SYS_FUTEX_WAKE,
            SYS_FUTEX_LOCK_PI,
            SYS_FUTEX_UNLOCK_PI,
            SYS_FUTEX_WAIT_TIMEOUT,
            SYS_EVENTFD_CREATE,
            SYS_EVENTFD_WRITE,
            SYS_EVENTFD_READ,
            SYS_EVENTFD_TRY_READ,
            SYS_EVENTFD_CLOSE,
            SYS_EVENTFD_READ_TIMEOUT,
            SYS_EVENTFD_WRITE_TIMEOUT,
            SYS_EVENTFD_HAS_VALUE,
            SYS_TCP_CONNECT,
            SYS_TCP_SEND,
            SYS_TCP_RECV,
            SYS_TCP_CLOSE,
            SYS_TCP_BIND,
            SYS_TCP_ACCEPT,
            SYS_TCP_CLOSE_LISTENER,
            SYS_TCP_ABORT,
            SYS_TCP_PEER_ADDR,
            SYS_UDP_BIND,
            SYS_UDP_SEND,
            SYS_UDP_RECV,
            SYS_UDP_CLOSE,
            SYS_UDP_MCAST_JOIN,
            SYS_UDP_MCAST_LEAVE,
            SYS_UDP_CONNECT,
            SYS_UDP_LOCAL_PORT,
            SYS_DNS_RESOLVE,
            SYS_DNS_REVERSE_RESOLVE,
            SYS_NET_STAT,
            SYS_ICMP_PING,
            SYS_ICMP_PING_WAIT,
            SYS_TCP_LIST,
            SYS_TCP_LISTENER_LIST,
            SYS_NET_IF_INFO,
            SYS_ARP_TABLE,
            SYS_DNS_CACHE_STATS,
            SYS_TCP_POLL_STATUS,
            SYS_TCP_LISTENER_READY,
            SYS_UDP_RX_READY,
            SYS_UDP_RX_FRONT_BYTES,
            SYS_TCP_SHUTDOWN,
            SYS_TCP_INFO,
            SYS_TCP_SET_NODELAY,
            SYS_TCP_SET_KEEPALIVE,
            SYS_TCP_SET_KEEPALIVE_PARAMS,
            SYS_TCP_LAST_ERROR,
            SYS_TCP_LOCAL_PORT,
        ];
        for i in 0..all_numbers.len() {
            for j in (i + 1)..all_numbers.len() {
                assert_ne!(
                    all_numbers[i], all_numbers[j],
                    "syscall numbers at indices {i} and {j} must be distinct (both = {})",
                    all_numbers[i]
                );
            }
        }
    }

    // -- Syscall number ranges match zone allocation --

    #[test]
    fn syscall_ranges_by_zone() {
        // kernel-core: 0-199
        assert!(SYS_EXIT <= 199);
        assert!(SYS_TASK_ID <= 199);
        assert!(SYS_CLOCK_MONOTONIC <= 199);
        assert!(SYS_SLEEP <= 199);
        assert!(SYS_CONSOLE_WRITE <= 199);
        assert!(SYS_CONSOLE_READ_CHAR <= 199);
        assert!(SYS_LOG_READ <= 199);
        assert!(SYS_MMAP <= 199);
        assert!(SYS_MUNMAP <= 199);
        assert!(SYS_MPROTECT <= 199);
        assert!(SYS_SCHED_SET_PROFILE <= 199);

        // kernel-ipc: 200-399
        assert!((200..400).contains(&SYS_PIPE_CREATE));
        assert!((200..400).contains(&SYS_PIPE_WRITE));
        assert!((200..400).contains(&SYS_PIPE_READ));
        assert!((200..400).contains(&SYS_PIPE_CLOSE));
        assert!((200..400).contains(&SYS_EVENTFD_CREATE));
        assert!((200..400).contains(&SYS_EVENTFD_WRITE));
        assert!((200..400).contains(&SYS_EVENTFD_READ));
        assert!((200..400).contains(&SYS_EVENTFD_CLOSE));

        // kernel-process: 500-599
        assert!((500..600).contains(&SYS_PROCESS_ID));
        assert!((500..600).contains(&SYS_PROCESS_SPAWN));
        assert!((500..600).contains(&SYS_PROCESS_WAIT));
        assert!((500..600).contains(&SYS_PROCESS_EXEC));
        assert!((500..600).contains(&SYS_THREAD_CREATE));
        assert!((500..600).contains(&SYS_THREAD_EXIT));
        assert!((500..600).contains(&SYS_THREAD_JOIN));
        assert!((500..600).contains(&SYS_PROCESS_SPAWN_EX));
        assert!((500..600).contains(&SYS_PROCESS_GET_INITIAL_FDS));
        assert!((500..600).contains(&SYS_PROCESS_GET_ARGS));

        // fs: 600-799
        assert!((600..800).contains(&SYS_FS_READ_FILE));
        assert!((600..800).contains(&SYS_FS_WRITE_FILE));
        assert!((600..800).contains(&SYS_FS_OPEN));
        assert!((600..800).contains(&SYS_FS_CLOSE));
        assert!((600..800).contains(&SYS_FS_DUP));

        // net: 800-999
        assert!((800..1000).contains(&SYS_TCP_CONNECT));
        assert!((800..1000).contains(&SYS_UDP_BIND));
        assert!((800..1000).contains(&SYS_DNS_RESOLVE));
    }

    // -- All IPC syscall numbers (pipe + eventfd) in IPC range --

    #[test]
    fn ipc_syscalls_in_ipc_range() {
        let ipc_nrs = [
            SYS_PIPE_CREATE,
            SYS_PIPE_WRITE,
            SYS_PIPE_READ,
            SYS_PIPE_TRY_WRITE,
            SYS_PIPE_TRY_READ,
            SYS_PIPE_CLOSE,
            SYS_PIPE_POLL,
            SYS_PIPE_READABLE_BYTES,
            SYS_FUTEX_WAIT,
            SYS_FUTEX_WAKE,
            SYS_FUTEX_LOCK_PI,
            SYS_FUTEX_UNLOCK_PI,
            SYS_FUTEX_WAIT_TIMEOUT,
            SYS_EVENTFD_CREATE,
            SYS_EVENTFD_WRITE,
            SYS_EVENTFD_READ,
            SYS_EVENTFD_TRY_READ,
            SYS_EVENTFD_CLOSE,
            SYS_EVENTFD_READ_TIMEOUT,
            SYS_EVENTFD_WRITE_TIMEOUT,
            SYS_EVENTFD_HAS_VALUE,
        ];
        for &nr in &ipc_nrs {
            assert!(
                (200..400).contains(&nr),
                "IPC syscall {nr} must be in IPC range 200-399"
            );
        }
    }

    // -- All TCP syscall numbers in net range --

    #[test]
    fn tcp_syscalls_in_net_range() {
        let tcp_nrs = [
            SYS_TCP_CONNECT,
            SYS_TCP_SEND,
            SYS_TCP_RECV,
            SYS_TCP_CLOSE,
            SYS_TCP_BIND,
            SYS_TCP_ACCEPT,
            SYS_TCP_CLOSE_LISTENER,
            SYS_TCP_ABORT,
            SYS_TCP_PEER_ADDR,
            SYS_TCP_POLL_STATUS,
            SYS_TCP_LISTENER_READY,
            SYS_TCP_SHUTDOWN,
            SYS_TCP_INFO,
            SYS_TCP_SET_NODELAY,
            SYS_TCP_SET_KEEPALIVE,
            SYS_TCP_SET_KEEPALIVE_PARAMS,
            SYS_TCP_LAST_ERROR,
            SYS_TCP_LOCAL_PORT,
            SYS_TCP_LIST,
            SYS_TCP_LISTENER_LIST,
        ];
        for &nr in &tcp_nrs {
            assert!(
                (800..1000).contains(&nr),
                "TCP syscall {nr} must be in net range 800-999"
            );
        }
    }

    // -- All UDP syscall numbers in net range --

    #[test]
    fn udp_syscalls_in_net_range() {
        let udp_nrs = [
            SYS_UDP_BIND,
            SYS_UDP_SEND,
            SYS_UDP_RECV,
            SYS_UDP_CLOSE,
            SYS_UDP_MCAST_JOIN,
            SYS_UDP_MCAST_LEAVE,
            SYS_UDP_CONNECT,
            SYS_UDP_LOCAL_PORT,
            SYS_UDP_RX_READY,
            SYS_UDP_RX_FRONT_BYTES,
        ];
        for &nr in &udp_nrs {
            assert!(
                (800..1000).contains(&nr),
                "UDP syscall {nr} must be in net range 800-999"
            );
        }
    }

    // -- DNS/ICMP/Net info syscalls in net range --

    #[test]
    fn dns_net_syscalls_in_net_range() {
        let nrs = [
            SYS_DNS_RESOLVE,
            SYS_DNS_REVERSE_RESOLVE,
            SYS_NET_STAT,
            SYS_ICMP_PING,
            SYS_ICMP_PING_WAIT,
            SYS_NET_IF_INFO,
            SYS_ARP_TABLE,
            SYS_DNS_CACHE_STATS,
        ];
        for &nr in &nrs {
            assert!(
                (800..1000).contains(&nr),
                "net info syscall {nr} must be in net range 800-999"
            );
        }
    }

    // -- All FS syscall numbers in fs range --

    #[test]
    fn fs_syscalls_in_fs_range() {
        let fs_nrs = [
            SYS_FS_READ_FILE,
            SYS_FS_WRITE_FILE,
            SYS_FS_DELETE,
            SYS_FS_LIST_DIR,
            SYS_FS_MKDIR,
            SYS_FS_RMDIR,
            SYS_FS_STAT,
            SYS_FS_LINK,
            SYS_FS_STATVFS,
            SYS_FS_OPEN,
            SYS_FS_CLOSE,
            SYS_FS_READ,
            SYS_FS_WRITE,
            SYS_FS_SEEK,
            SYS_FS_TRUNCATE,
            SYS_FS_RENAME,
            SYS_FS_FSTAT,
            SYS_FS_DUP,
            SYS_FS_COPY,
            SYS_FS_APPEND,
            SYS_FS_FTRUNCATE,
            SYS_FS_SYMLINK,
            SYS_FS_READLINK,
            SYS_FS_LSTAT,
            SYS_FS_SYNC,
            SYS_FS_FLOCK,
            SYS_FS_FUNLOCK,
            SYS_FS_SEEK_DATA,
            SYS_FS_SEEK_HOLE,
            SYS_FS_WATCH_CREATE,
            SYS_FS_WATCH_READ,
            SYS_FS_WATCH_CLOSE,
            SYS_FS_SET_TIMES,
            SYS_FS_SET_OWNER,
            SYS_FS_SET_PERMS,
            SYS_FS_GET_XATTR,
            SYS_FS_SET_XATTR,
            SYS_FS_REMOVE_XATTR,
            SYS_FS_LIST_XATTRS,
        ];
        for &nr in &fs_nrs {
            assert!(
                (600..800).contains(&nr),
                "FS syscall {nr} must be in fs range 600-799"
            );
        }
    }

    // -- Memory syscalls in kernel-core range --

    #[test]
    fn memory_syscalls_in_core_range() {
        assert!(SYS_MMAP <= 199);
        assert!(SYS_MUNMAP <= 199);
        assert!(SYS_MPROTECT <= 199);
    }

    // -- Host-build safety gate --
    //
    // On host builds (`not(target_os = "none")`), every `syscallN()`
    // returns -ENOSYS rather than emitting a real SYSCALL instruction.
    // These tests pin that contract so a future refactor cannot
    // regress us into executing UB against NT system services on the
    // Windows test host.

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_enosys_matches_errno_module() {
        // If `errno::ENOSYS` ever changes, `HOST_ENOSYS` must move with
        // it.  Pin both ends here.
        assert_eq!(HOST_ENOSYS, -(crate::errno::ENOSYS as i64));
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall0_returns_enosys() {
        assert_eq!(syscall0(SYS_EXIT), HOST_ENOSYS);
    }

    // The clock syscalls are special-cased inside `syscall0` so that
    // time-dependent code paths (timeouts, timestamps) work in host
    // tests.  Pin the contract: they must NOT return the ENOSYS
    // sentinel and they must return non-negative, non-decreasing
    // monotonic-ns / non-zero realtime-ns values.
    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall0_clock_monotonic_returns_non_decreasing_nanos() {
        let a = syscall0(SYS_CLOCK_MONOTONIC);
        let b = syscall0(SYS_CLOCK_MONOTONIC);
        assert!(a >= 0, "monotonic must be non-negative, got {a}");
        assert!(b >= a, "monotonic must be non-decreasing ({a} -> {b})");
        assert_ne!(a, HOST_ENOSYS);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall0_clock_realtime_returns_post_epoch_nanos() {
        let ns = syscall0(SYS_CLOCK_REALTIME);
        assert!(ns >= 0, "realtime must be non-negative, got {ns}");
        assert_ne!(ns, HOST_ENOSYS);
        // 2020-01-01 UTC was ~1577836800 s = 1.577e18 ns since epoch.
        // Anything older than that on a host running these tests is
        // a broken clock.  Lower bound chosen conservatively.
        const YEAR_2020_NS: i64 = 1_577_836_800_000_000_000;
        assert!(ns >= YEAR_2020_NS, "realtime clock looks unset: {ns}");
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall1_returns_enosys() {
        assert_eq!(syscall1(SYS_EXIT, 0), HOST_ENOSYS);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall2_returns_enosys() {
        assert_eq!(syscall2(SYS_EVENTFD_CREATE, 0, 0), HOST_ENOSYS);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall3_returns_enosys() {
        assert_eq!(syscall3(SYS_FS_READ, 0, 0, 0), HOST_ENOSYS);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall3_2ret_returns_enosys_and_zero() {
        assert_eq!(syscall3_2ret(SYS_LOG_READ, 0, 0, 0), (HOST_ENOSYS, 0));
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall4_returns_enosys() {
        assert_eq!(syscall4(SYS_FS_LINK, 0, 0, 0, 0), HOST_ENOSYS);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall5_returns_enosys() {
        assert_eq!(syscall5(SYS_MMAP, 0, 0, 0, 0, 0), HOST_ENOSYS);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall6_returns_enosys() {
        assert_eq!(syscall6(SYS_MMAP, 0, 0, 0, 0, 0, 0), HOST_ENOSYS);
    }
}
