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
pub const SYS_PROCESS_ID: u64 = 3;
pub const SYS_CLOCK_MONOTONIC: u64 = 10;
pub const SYS_SLEEP: u64 = 11;

// Console I/O
pub const SYS_CONSOLE_WRITE: u64 = 100;
pub const SYS_CONSOLE_READ_CHAR: u64 = 101;

// Memory management
pub const SYS_MMAP: u64 = 30;
pub const SYS_MUNMAP: u64 = 31;
pub const SYS_MPROTECT: u64 = 32;

// Scheduler / thread
pub const SYS_SCHED_SET_PROFILE: u64 = 53;

// Process management
pub const SYS_PROCESS_SPAWN: u64 = 500;
pub const SYS_PROCESS_WAIT: u64 = 501;
pub const SYS_PROCESS_EXEC: u64 = 503;
pub const SYS_PROCESS_TRY_WAIT: u64 = 507;
pub const SYS_THREAD_CREATE: u64 = 510;
pub const SYS_THREAD_EXIT: u64 = 511;
pub const SYS_THREAD_JOIN: u64 = 512;

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
pub const SYS_FS_OPEN: u64 = 610;
pub const SYS_FS_CLOSE: u64 = 611;
pub const SYS_FS_READ: u64 = 612;
pub const SYS_FS_WRITE: u64 = 613;
pub const SYS_FS_SEEK: u64 = 614;
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

// Sync
pub const SYS_FS_SYNC: u64 = 641;

// Pipes (IPC range 200-399)
pub const SYS_PIPE_CREATE: u64 = 220;
pub const SYS_PIPE_WRITE: u64 = 221;
pub const SYS_PIPE_READ: u64 = 222;
pub const SYS_PIPE_TRY_WRITE: u64 = 223;
pub const SYS_PIPE_TRY_READ: u64 = 224;
pub const SYS_PIPE_CLOSE: u64 = 225;
pub const SYS_PIPE_POLL: u64 = 228;
pub const SYS_PIPE_READABLE_BYTES: u64 = 229;

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

/// Issue a syscall with 0 arguments.
#[inline(always)]
#[must_use]
pub fn syscall0(nr: u64) -> i64 {
    let ret: i64;
    // SAFETY: The SYSCALL instruction is the defined kernel entry point.
    // RCX and R11 are clobbered by SYSCALL (saves RIP and RFLAGS).
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

/// Issue a syscall with 1 argument.
#[inline(always)]
#[must_use]
pub fn syscall1(nr: u64, arg0: u64) -> i64 {
    let ret: i64;
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

/// Issue a syscall with 2 arguments.
#[inline(always)]
#[must_use]
pub fn syscall2(nr: u64, arg0: u64, arg1: u64) -> i64 {
    let ret: i64;
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

/// Issue a syscall with 3 arguments.
#[inline(always)]
#[must_use]
pub fn syscall3(nr: u64, arg0: u64, arg1: u64, arg2: u64) -> i64 {
    let ret: i64;
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

/// Issue a syscall with 4 arguments.
#[inline(always)]
#[must_use]
pub fn syscall4(nr: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    let ret: i64;
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

/// Issue a syscall with 5 arguments.
#[inline(always)]
#[must_use]
pub fn syscall5(nr: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64) -> i64 {
    let ret: i64;
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

/// Issue a syscall with 6 arguments.
#[inline(always)]
#[must_use]
pub fn syscall6(
    nr: u64, arg0: u64, arg1: u64, arg2: u64,
    arg3: u64, arg4: u64, arg5: u64,
) -> i64 {
    let ret: i64;
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
