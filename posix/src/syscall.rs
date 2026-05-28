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
            SYS_EXIT, SYS_TASK_ID, SYS_PROCESS_ID,
            SYS_CLOCK_MONOTONIC, SYS_SLEEP,
            SYS_CONSOLE_WRITE, SYS_CONSOLE_READ_CHAR,
            SYS_MMAP, SYS_MUNMAP, SYS_MPROTECT,
            SYS_SCHED_SET_PROFILE,
            SYS_PROCESS_SPAWN, SYS_PROCESS_WAIT, SYS_PROCESS_EXEC,
            SYS_PROCESS_TRY_WAIT,
            SYS_THREAD_CREATE, SYS_THREAD_EXIT, SYS_THREAD_JOIN,
            SYS_PROCESS_SPAWN_EX, SYS_PROCESS_GET_INITIAL_FDS,
            SYS_PROCESS_GET_ARGS,
            SYS_FS_READ_FILE, SYS_FS_WRITE_FILE, SYS_FS_DELETE,
            SYS_FS_LIST_DIR, SYS_FS_MKDIR, SYS_FS_RMDIR,
            SYS_FS_STAT, SYS_FS_LINK, SYS_FS_STATVFS,
            SYS_FS_OPEN, SYS_FS_CLOSE, SYS_FS_READ, SYS_FS_WRITE,
            SYS_FS_SEEK, SYS_FS_TRUNCATE, SYS_FS_RENAME, SYS_FS_FSTAT,
            SYS_FS_DUP, SYS_FS_COPY, SYS_FS_APPEND, SYS_FS_FTRUNCATE,
            SYS_FS_SYMLINK, SYS_FS_READLINK, SYS_FS_LSTAT, SYS_FS_SYNC,
            SYS_PIPE_CREATE, SYS_PIPE_WRITE, SYS_PIPE_READ,
            SYS_PIPE_TRY_WRITE, SYS_PIPE_TRY_READ, SYS_PIPE_CLOSE,
            SYS_PIPE_POLL, SYS_PIPE_READABLE_BYTES,
            SYS_FUTEX_WAIT, SYS_FUTEX_WAKE, SYS_FUTEX_LOCK_PI,
            SYS_FUTEX_UNLOCK_PI, SYS_FUTEX_WAIT_TIMEOUT,
            SYS_EVENTFD_CREATE, SYS_EVENTFD_WRITE, SYS_EVENTFD_READ,
            SYS_EVENTFD_TRY_READ, SYS_EVENTFD_CLOSE,
            SYS_EVENTFD_READ_TIMEOUT, SYS_EVENTFD_WRITE_TIMEOUT,
            SYS_EVENTFD_HAS_VALUE,
            SYS_TCP_CONNECT, SYS_TCP_SEND, SYS_TCP_RECV, SYS_TCP_CLOSE,
            SYS_TCP_BIND, SYS_TCP_ACCEPT, SYS_TCP_CLOSE_LISTENER,
            SYS_TCP_ABORT, SYS_TCP_PEER_ADDR,
            SYS_UDP_BIND, SYS_UDP_SEND, SYS_UDP_RECV, SYS_UDP_CLOSE,
            SYS_UDP_MCAST_JOIN, SYS_UDP_MCAST_LEAVE, SYS_UDP_CONNECT,
            SYS_UDP_LOCAL_PORT,
            SYS_DNS_RESOLVE, SYS_DNS_REVERSE_RESOLVE, SYS_NET_STAT,
            SYS_ICMP_PING, SYS_ICMP_PING_WAIT,
            SYS_TCP_LIST, SYS_TCP_LISTENER_LIST,
            SYS_NET_IF_INFO, SYS_ARP_TABLE, SYS_DNS_CACHE_STATS,
            SYS_TCP_POLL_STATUS, SYS_TCP_LISTENER_READY,
            SYS_UDP_RX_READY, SYS_UDP_RX_FRONT_BYTES,
            SYS_TCP_SHUTDOWN, SYS_TCP_INFO, SYS_TCP_SET_NODELAY,
            SYS_TCP_SET_KEEPALIVE, SYS_TCP_SET_KEEPALIVE_PARAMS,
            SYS_TCP_LAST_ERROR, SYS_TCP_LOCAL_PORT,
        ];
        for &nr in &all_numbers {
            assert_ne!(nr, 0, "syscall number must not be zero");
        }
    }

    // -- All syscall numbers are unique --

    #[test]
    fn syscall_numbers_unique() {
        let all_numbers: &[u64] = &[
            SYS_EXIT, SYS_TASK_ID, SYS_PROCESS_ID,
            SYS_CLOCK_MONOTONIC, SYS_SLEEP,
            SYS_CONSOLE_WRITE, SYS_CONSOLE_READ_CHAR,
            SYS_MMAP, SYS_MUNMAP, SYS_MPROTECT,
            SYS_SCHED_SET_PROFILE,
            SYS_PROCESS_SPAWN, SYS_PROCESS_WAIT, SYS_PROCESS_EXEC,
            SYS_PROCESS_TRY_WAIT,
            SYS_THREAD_CREATE, SYS_THREAD_EXIT, SYS_THREAD_JOIN,
            SYS_PROCESS_SPAWN_EX, SYS_PROCESS_GET_INITIAL_FDS,
            SYS_PROCESS_GET_ARGS,
            SYS_FS_READ_FILE, SYS_FS_WRITE_FILE, SYS_FS_DELETE,
            SYS_FS_LIST_DIR, SYS_FS_MKDIR, SYS_FS_RMDIR,
            SYS_FS_STAT, SYS_FS_LINK, SYS_FS_STATVFS,
            SYS_FS_OPEN, SYS_FS_CLOSE, SYS_FS_READ, SYS_FS_WRITE,
            SYS_FS_SEEK, SYS_FS_TRUNCATE, SYS_FS_RENAME, SYS_FS_FSTAT,
            SYS_FS_DUP, SYS_FS_COPY, SYS_FS_APPEND, SYS_FS_FTRUNCATE,
            SYS_FS_SYMLINK, SYS_FS_READLINK, SYS_FS_LSTAT, SYS_FS_SYNC,
            SYS_PIPE_CREATE, SYS_PIPE_WRITE, SYS_PIPE_READ,
            SYS_PIPE_TRY_WRITE, SYS_PIPE_TRY_READ, SYS_PIPE_CLOSE,
            SYS_PIPE_POLL, SYS_PIPE_READABLE_BYTES,
            SYS_FUTEX_WAIT, SYS_FUTEX_WAKE, SYS_FUTEX_LOCK_PI,
            SYS_FUTEX_UNLOCK_PI, SYS_FUTEX_WAIT_TIMEOUT,
            SYS_EVENTFD_CREATE, SYS_EVENTFD_WRITE, SYS_EVENTFD_READ,
            SYS_EVENTFD_TRY_READ, SYS_EVENTFD_CLOSE,
            SYS_EVENTFD_READ_TIMEOUT, SYS_EVENTFD_WRITE_TIMEOUT,
            SYS_EVENTFD_HAS_VALUE,
            SYS_TCP_CONNECT, SYS_TCP_SEND, SYS_TCP_RECV, SYS_TCP_CLOSE,
            SYS_TCP_BIND, SYS_TCP_ACCEPT, SYS_TCP_CLOSE_LISTENER,
            SYS_TCP_ABORT, SYS_TCP_PEER_ADDR,
            SYS_UDP_BIND, SYS_UDP_SEND, SYS_UDP_RECV, SYS_UDP_CLOSE,
            SYS_UDP_MCAST_JOIN, SYS_UDP_MCAST_LEAVE, SYS_UDP_CONNECT,
            SYS_UDP_LOCAL_PORT,
            SYS_DNS_RESOLVE, SYS_DNS_REVERSE_RESOLVE, SYS_NET_STAT,
            SYS_ICMP_PING, SYS_ICMP_PING_WAIT,
            SYS_TCP_LIST, SYS_TCP_LISTENER_LIST,
            SYS_NET_IF_INFO, SYS_ARP_TABLE, SYS_DNS_CACHE_STATS,
            SYS_TCP_POLL_STATUS, SYS_TCP_LISTENER_READY,
            SYS_UDP_RX_READY, SYS_UDP_RX_FRONT_BYTES,
            SYS_TCP_SHUTDOWN, SYS_TCP_INFO, SYS_TCP_SET_NODELAY,
            SYS_TCP_SET_KEEPALIVE, SYS_TCP_SET_KEEPALIVE_PARAMS,
            SYS_TCP_LAST_ERROR, SYS_TCP_LOCAL_PORT,
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
        assert!(SYS_PROCESS_ID <= 199);
        assert!(SYS_CLOCK_MONOTONIC <= 199);
        assert!(SYS_SLEEP <= 199);
        assert!(SYS_CONSOLE_WRITE <= 199);
        assert!(SYS_CONSOLE_READ_CHAR <= 199);
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
            SYS_PIPE_CREATE, SYS_PIPE_WRITE, SYS_PIPE_READ,
            SYS_PIPE_TRY_WRITE, SYS_PIPE_TRY_READ, SYS_PIPE_CLOSE,
            SYS_PIPE_POLL, SYS_PIPE_READABLE_BYTES,
            SYS_FUTEX_WAIT, SYS_FUTEX_WAKE, SYS_FUTEX_LOCK_PI,
            SYS_FUTEX_UNLOCK_PI, SYS_FUTEX_WAIT_TIMEOUT,
            SYS_EVENTFD_CREATE, SYS_EVENTFD_WRITE, SYS_EVENTFD_READ,
            SYS_EVENTFD_TRY_READ, SYS_EVENTFD_CLOSE,
            SYS_EVENTFD_READ_TIMEOUT, SYS_EVENTFD_WRITE_TIMEOUT,
            SYS_EVENTFD_HAS_VALUE,
        ];
        for &nr in &ipc_nrs {
            assert!((200..400).contains(&nr),
                "IPC syscall {nr} must be in IPC range 200-399");
        }
    }

    // -- All TCP syscall numbers in net range --

    #[test]
    fn tcp_syscalls_in_net_range() {
        let tcp_nrs = [
            SYS_TCP_CONNECT, SYS_TCP_SEND, SYS_TCP_RECV, SYS_TCP_CLOSE,
            SYS_TCP_BIND, SYS_TCP_ACCEPT, SYS_TCP_CLOSE_LISTENER,
            SYS_TCP_ABORT, SYS_TCP_PEER_ADDR,
            SYS_TCP_POLL_STATUS, SYS_TCP_LISTENER_READY,
            SYS_TCP_SHUTDOWN, SYS_TCP_INFO, SYS_TCP_SET_NODELAY,
            SYS_TCP_SET_KEEPALIVE, SYS_TCP_SET_KEEPALIVE_PARAMS,
            SYS_TCP_LAST_ERROR, SYS_TCP_LOCAL_PORT,
            SYS_TCP_LIST, SYS_TCP_LISTENER_LIST,
        ];
        for &nr in &tcp_nrs {
            assert!((800..1000).contains(&nr),
                "TCP syscall {nr} must be in net range 800-999");
        }
    }

    // -- All UDP syscall numbers in net range --

    #[test]
    fn udp_syscalls_in_net_range() {
        let udp_nrs = [
            SYS_UDP_BIND, SYS_UDP_SEND, SYS_UDP_RECV, SYS_UDP_CLOSE,
            SYS_UDP_MCAST_JOIN, SYS_UDP_MCAST_LEAVE, SYS_UDP_CONNECT,
            SYS_UDP_LOCAL_PORT, SYS_UDP_RX_READY, SYS_UDP_RX_FRONT_BYTES,
        ];
        for &nr in &udp_nrs {
            assert!((800..1000).contains(&nr),
                "UDP syscall {nr} must be in net range 800-999");
        }
    }

    // -- DNS/ICMP/Net info syscalls in net range --

    #[test]
    fn dns_net_syscalls_in_net_range() {
        let nrs = [
            SYS_DNS_RESOLVE, SYS_DNS_REVERSE_RESOLVE,
            SYS_NET_STAT, SYS_ICMP_PING, SYS_ICMP_PING_WAIT,
            SYS_NET_IF_INFO, SYS_ARP_TABLE, SYS_DNS_CACHE_STATS,
        ];
        for &nr in &nrs {
            assert!((800..1000).contains(&nr),
                "net info syscall {nr} must be in net range 800-999");
        }
    }

    // -- All FS syscall numbers in fs range --

    #[test]
    fn fs_syscalls_in_fs_range() {
        let fs_nrs = [
            SYS_FS_READ_FILE, SYS_FS_WRITE_FILE, SYS_FS_DELETE,
            SYS_FS_LIST_DIR, SYS_FS_MKDIR, SYS_FS_RMDIR,
            SYS_FS_STAT, SYS_FS_LINK, SYS_FS_STATVFS,
            SYS_FS_OPEN, SYS_FS_CLOSE, SYS_FS_READ, SYS_FS_WRITE,
            SYS_FS_SEEK, SYS_FS_TRUNCATE, SYS_FS_RENAME, SYS_FS_FSTAT,
            SYS_FS_DUP, SYS_FS_COPY, SYS_FS_APPEND, SYS_FS_FTRUNCATE,
            SYS_FS_SYMLINK, SYS_FS_READLINK, SYS_FS_LSTAT, SYS_FS_SYNC,
        ];
        for &nr in &fs_nrs {
            assert!((600..800).contains(&nr),
                "FS syscall {nr} must be in fs range 600-799");
        }
    }

    // -- Memory syscalls in kernel-core range --

    #[test]
    fn memory_syscalls_in_core_range() {
        assert!(SYS_MMAP <= 199);
        assert!(SYS_MUNMAP <= 199);
        assert!(SYS_MPROTECT <= 199);
    }
}
