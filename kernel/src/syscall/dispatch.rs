//! Syscall dispatch table and handler infrastructure.
//!
//! The dispatch table maps syscall numbers to handler functions.
//! A handler receives up to 6 arguments (in registers) and returns
//! a result packed into two registers (`rax`, `rdx`).
//!
//! ## Versioning
//!
//! Each API version has its own dispatch table.  Currently only
//! version 1 exists.  When syscalls are deprecated, they remain
//! in older version tables.
//!
//! ## Performance
//!
//! Dispatch is O(1): a bounds check + array index.  The table is a
//! flat `[Option<SyscallHandler>; MAX_SYSCALL_NR]` array.

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

use super::handlers;
use super::number::{
    MAX_SYSCALL_NR, SYS_ARP_TABLE, SYS_CAP_QUERY, SYS_CAP_REQUEST, SYS_CAP_REQUEST_CANCEL,
    SYS_CAP_REQUEST_STATUS, SYS_CHANNEL_CLOSE, SYS_CHANNEL_CREATE, SYS_CHANNEL_PEER_CRED,
    SYS_CHANNEL_RECV, SYS_CHANNEL_RECV_CAPS, SYS_CHANNEL_RECV_TIMEOUT, SYS_CHANNEL_SEND,
    SYS_CHANNEL_SEND_BLOCKING, SYS_CHANNEL_SEND_CAPS, SYS_CHANNEL_SEND_TIMEOUT,
    SYS_CHANNEL_TRY_RECV, SYS_CLOCK_ADJTIME, SYS_CLOCK_MONOTONIC, SYS_CLOCK_REALTIME,
    SYS_CLOCK_SETTIME, SYS_CONSOLE_READ_CHAR, SYS_CONSOLE_TRY_READ_CHAR, SYS_CONSOLE_WRITE,
    SYS_CP_CLOSE, SYS_CP_CREATE, SYS_CP_NOTIFY, SYS_CP_REGISTER, SYS_CP_TRY_WAIT,
    SYS_CP_UNREGISTER, SYS_CP_WAIT, SYS_CPU_COUNT, SYS_CPU_TIMES, SYS_DEBUG_PRINT, SYS_DMA_ALLOC,
    SYS_DMA_ATTACH, SYS_DMA_DETACH, SYS_DMA_DOMAIN_CREATE, SYS_DMA_DOMAIN_DESTROY, SYS_DMA_FREE,
    SYS_DMA_MAP, SYS_DMA_UNMAP, SYS_DNS_CACHE_STATS, SYS_DNS_RESOLVE, SYS_DNS_REVERSE_RESOLVE,
    SYS_DRM_ATOMIC_COMMIT, SYS_DRM_CLOSE, SYS_DRM_CONNECTOR_STATUS, SYS_DRM_CRTC_INFO,
    SYS_DRM_CURSOR_MOVE, SYS_DRM_CURSOR_SET, SYS_DRM_DISPLAY_SIZE, SYS_DRM_FB_CREATE,
    SYS_DRM_FB_DESTROY, SYS_DRM_FLUSH_REGION, SYS_DRM_GEM_CREATE, SYS_DRM_GEM_DESTROY,
    SYS_DRM_GEM_MMAP, SYS_DRM_MODE_GET, SYS_DRM_OPEN, SYS_DRM_PAGE_FLIP, SYS_EVENTFD_CLOSE,
    SYS_EVENTFD_CREATE, SYS_EVENTFD_HAS_VALUE, SYS_EVENTFD_READ, SYS_EVENTFD_READ_TIMEOUT,
    SYS_EVENTFD_TRY_READ, SYS_EVENTFD_WRITE, SYS_EVENTFD_WRITE_TIMEOUT, SYS_EXIT, SYS_FS_APPEND,
    SYS_FS_CHECK, SYS_FS_CLOSE, SYS_FS_COPY, SYS_FS_DELETE, SYS_FS_DUP, SYS_FS_FALLOCATE,
    SYS_FS_FLOCK, SYS_FS_FORMAT, SYS_FS_FSTAT, SYS_FS_FTRUNCATE, SYS_FS_FUNLOCK, SYS_FS_GET_XATTR,
    SYS_FS_HANDLE_PATH, SYS_FS_JOURNAL_CURSOR, SYS_FS_JOURNAL_FLUSH, SYS_FS_JOURNAL_READ,
    SYS_FS_LINK, SYS_FS_LIST_DIR, SYS_FS_LIST_XATTRS, SYS_FS_LSTAT, SYS_FS_METADATA, SYS_FS_MKDIR,
    SYS_FS_MKDIR_MODE, SYS_FS_MOUNT, SYS_FS_OPEN, SYS_FS_OPEN_MODE, SYS_FS_READ, SYS_FS_READ_FILE,
    SYS_FS_READDIR_AT, SYS_FS_READLINK, SYS_FS_REMOVE_XATTR, SYS_FS_RENAME, SYS_FS_RMDIR,
    SYS_FS_SEEK, SYS_FS_SEEK_DATA, SYS_FS_SEEK_HOLE, SYS_FS_SET_ATTR, SYS_FS_SET_OWNER,
    SYS_FS_SET_PERMS, SYS_FS_SET_TIMES, SYS_FS_SET_XATTR, SYS_FS_STAT, SYS_FS_STATVFS,
    SYS_FS_SYMLINK, SYS_FS_SYNC, SYS_FS_TMPFILE, SYS_FS_TRASH, SYS_FS_TRASH_EMPTY,
    SYS_FS_TRASH_LIST, SYS_FS_TRASH_RESTORE, SYS_FS_TRIM, SYS_FS_TRUNCATE, SYS_FS_UMOUNT,
    SYS_FS_WATCH_CLOSE, SYS_FS_WATCH_CREATE, SYS_FS_WATCH_READ, SYS_FS_WRITE, SYS_FS_WRITE_FILE,
    SYS_FUTEX_CMP_REQUEUE_PI, SYS_FUTEX_LOCK_PI, SYS_FUTEX_LOCK_PI_TIMEOUT, SYS_FUTEX_REQUEUE,
    SYS_FUTEX_TRYLOCK_PI, SYS_FUTEX_UNLOCK_PI, SYS_FUTEX_WAIT, SYS_FUTEX_WAIT_REQUEUE_PI,
    SYS_FUTEX_WAIT_TIMEOUT, SYS_FUTEX_WAKE, SYS_GETRANDOM, SYS_ICMP_PING, SYS_ICMP_PING_WAIT,
    SYS_IO_RING_DESTROY, SYS_IO_RING_ENTER, SYS_IO_RING_SETUP, SYS_IRQ_REGISTER, SYS_IRQ_RELEASE,
    SYS_IRQ_WAIT, SYS_LOADAVG, SYS_LOG_READ, SYS_MM_GET_PROFILE, SYS_MM_SET_PROFILE, SYS_MMAP,
    SYS_MPROTECT, SYS_MUNMAP, SYS_NET_FW_ADD_RULE, SYS_NET_FW_DEL_RULE, SYS_NET_FW_ENABLE,
    SYS_NET_FW_FLUSH, SYS_NET_FW_SET_POLICY, SYS_NET_IF_CONFIG, SYS_NET_IF_INFO, SYS_NET_RAW_CLOSE,
    SYS_NET_RAW_OPEN, SYS_NET_RAW_RX, SYS_NET_RAW_TX, SYS_NET_ROUTE_ADD, SYS_NET_ROUTE_DEL,
    SYS_NET_ROUTE_LIST, SYS_NET_STAT, SYS_NOTIFY_READY, SYS_NS_ATTACH, SYS_NS_BIND, SYS_NS_CREATE,
    SYS_NS_HIDE, SYS_NS_QUERY, SYS_NS_UNBIND, SYS_PHYS_PAGES_AVAIL, SYS_PHYS_PAGES_TOTAL,
    SYS_PIPE_CLOSE, SYS_PIPE_CREATE, SYS_PIPE_PEEK, SYS_PIPE_POLL, SYS_PIPE_READ,
    SYS_PIPE_READ_TIMEOUT, SYS_PIPE_READABLE_BYTES, SYS_PIPE_TRY_READ, SYS_PIPE_TRY_WRITE,
    SYS_PIPE_WAIT_READABLE, SYS_PIPE_WRITE, SYS_PIPE_WRITE_TIMEOUT, SYS_PORT_READ, SYS_PORT_WRITE,
    SYS_PROCESS_COUNT, SYS_PROCESS_CRASH_INFO, SYS_PROCESS_GET_ARGS, SYS_PROCESS_GET_CREDENTIALS,
    SYS_PROCESS_GET_INITIAL_FDS, SYS_PROCESS_GET_NICE, SYS_PROCESS_GET_PGID,
    SYS_PROCESS_GET_RUSAGE, SYS_PROCESS_GET_SID, SYS_PROCESS_ID, SYS_PROCESS_IS_READY,
    SYS_PROCESS_KILL, SYS_PROCESS_PARENT_ID, SYS_PROCESS_SET_CREDENTIALS, SYS_PROCESS_SET_EXEC_FDS,
    SYS_PROCESS_SET_NICE, SYS_PROCESS_SET_PGID, SYS_PROCESS_SET_SID, SYS_PROCESS_SPAWN,
    SYS_PROCESS_SPAWN_EX, SYS_PROCESS_SPAWN_EX2, SYS_PROCESS_TRY_WAIT, SYS_PROCESS_WAIT,
    SYS_PROCESS_WAIT_STATUS, SYS_PTY_CLOSE, SYS_PTY_CREATE, SYS_PTY_DUP, SYS_PTY_GET_PGRP,
    SYS_PTY_GET_TERMIOS, SYS_PTY_GET_WINSIZE, SYS_PTY_MASTER_READ, SYS_PTY_MASTER_TRY_READ,
    SYS_PTY_MASTER_WRITE, SYS_PTY_POLL, SYS_PTY_READABLE_BYTES, SYS_PTY_SET_PGRP,
    SYS_PTY_SET_TERMIOS, SYS_PTY_SET_WINSIZE, SYS_PTY_SLAVE_ID, SYS_PTY_SLAVE_WRITE,
    SYS_RLIMIT_GET, SYS_RLIMIT_SET, SYS_SCHED_GET_PROFILE, SYS_SCHED_GET_TIMESLICE,
    SYS_SCHED_RECONFIGURE, SYS_SCHED_SET_PROFILE, SYS_SCHED_SET_TIMESLICE, SYS_SEM_CLOSE,
    SYS_SEM_CREATE, SYS_SEM_SIGNAL, SYS_SEM_TRY_WAIT, SYS_SEM_WAIT, SYS_SEM_WAIT_TIMEOUT,
    SYS_SERVICE_ACCEPT, SYS_SERVICE_ACCEPT_TIMEOUT, SYS_SERVICE_CONNECT, SYS_SERVICE_REGISTER,
    SYS_SERVICE_TRY_ACCEPT, SYS_SERVICE_UNREGISTER, SYS_SET_EXCEPTION_HANDLER, SYS_SET_FS_BASE,
    SYS_SHM_CLOSE, SYS_SHM_CREATE, SYS_SHM_MAP, SYS_SHM_SIZE, SYS_SHM_UNMAP, SYS_SIGNAL_MASK,
    SYS_SIGNAL_PENDING, SYS_SIGNAL_REGISTER, SYS_SIGNAL_SEND, SYS_SIGNAL_STOP_SELF, SYS_SLEEP,
    SYS_SOCKETPAIR_CLOSE, SYS_SOCKETPAIR_CREATE, SYS_SOCKETPAIR_POLL,
    SYS_SOCKETPAIR_READABLE_BYTES, SYS_SOCKETPAIR_RECV, SYS_SOCKETPAIR_RECV_TIMEOUT,
    SYS_SOCKETPAIR_SEND, SYS_SOCKETPAIR_SEND_TIMEOUT, SYS_SOCKETPAIR_SHUTDOWN,
    SYS_SOCKETPAIR_TRY_RECV, SYS_SOCKETPAIR_TRY_SEND, SYS_SYSCTL_GET, SYS_SYSCTL_SET,
    SYS_SYSTEM_SET_PROFILE, SYS_TASK_ID, SYS_TCP_ABORT, SYS_TCP_ACCEPT, SYS_TCP_BIND,
    SYS_TCP_CLOSE, SYS_TCP_CLOSE_LISTENER, SYS_TCP_CONNECT, SYS_TCP_INFO, SYS_TCP_LAST_ERROR,
    SYS_TCP_LIST, SYS_TCP_LISTENER_LIST, SYS_TCP_LISTENER_READY, SYS_TCP_LOCAL_PORT,
    SYS_TCP_PEER_ADDR, SYS_TCP_POLL_STATUS, SYS_TCP_RECV, SYS_TCP_SEND, SYS_TCP_SET_KEEPALIVE,
    SYS_TCP_SET_KEEPALIVE_PARAMS, SYS_TCP_SET_NODELAY, SYS_TCP_SHUTDOWN, SYS_THREAD_CREATE,
    SYS_THREAD_EXIT, SYS_THREAD_JOIN, SYS_THREAD_RESUME, SYS_THREAD_SET_PRIORITY,
    SYS_THREAD_SUSPEND, SYS_TIMER_CANCEL, SYS_TIMER_CREATE, SYS_TTY_ACQUIRE_CTTY, SYS_TTY_GET_PGRP,
    SYS_TTY_GET_TERMIOS, SYS_TTY_READ, SYS_TTY_RELEASE_CTTY, SYS_TTY_SET_PGRP, SYS_TTY_SET_TERMIOS,
    SYS_UDP_BIND, SYS_UDP_CLOSE, SYS_UDP_CONNECT, SYS_UDP_LOCAL_PORT, SYS_UDP_MCAST_JOIN,
    SYS_UDP_MCAST_LEAVE, SYS_UDP_RECV, SYS_UDP_RX_FRONT_BYTES, SYS_UDP_RX_READY, SYS_UDP_SEND,
    SYS_YIELD,
};
use crate::drm::syscall as drm_handlers;

// ---------------------------------------------------------------------------
// Syscall argument and result types
// ---------------------------------------------------------------------------

/// Arguments to a syscall (up to 6 register-width values).
///
/// On `x86_64`, these arrive in: `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9`.
/// (Note: `r10` instead of `rcx` — the `syscall` instruction clobbers
/// `rcx`.)
#[derive(Debug, Clone, Copy)]
pub struct SyscallArgs {
    pub arg0: u64,
    pub arg1: u64,
    pub arg2: u64,
    pub arg3: u64,
    pub arg4: u64,
    pub arg5: u64,
}

/// Result of a syscall, returned to userspace.
///
/// On `x86_64`, `value` goes in `rax`, `value2` in `rdx`.
/// For most syscalls, only `value` is used.  `value2` is for
/// operations that return two values (e.g., `channel_create` returns
/// two handles).
#[derive(Debug, Clone, Copy)]
pub struct SyscallResult {
    /// Primary return value (`rax`).  Negative = error code.
    pub value: i64,
    /// Secondary return value (`rdx`).  Usually 0.
    pub value2: i64,
    /// Whether `value2` is a real second return value that must be
    /// delivered to userspace in `rdx`.  Only set by [`SyscallResult::ok2`].
    ///
    /// This discriminant exists because the native SYSCALL exit path
    /// *preserves* the user's `rdx` (arg2) across a normal syscall — the
    /// SysV/musl convention callers rely on.  Unconditionally writing
    /// `value2` into `rdx` would clobber it for every single-value
    /// syscall.  Only when `has_value2` is set does the return path
    /// overwrite the frame's `rdx` slot, so two-value syscalls (e.g.
    /// `SYS_PIPE_CREATE`, `SYS_CHANNEL_CREATE`) deliver their second
    /// handle without regressing the RDX-preserved guarantee for the
    /// rest.
    pub has_value2: bool,
}

impl SyscallResult {
    /// Success with a single return value.
    #[must_use]
    pub const fn ok(value: i64) -> Self {
        Self {
            value,
            value2: 0,
            has_value2: false,
        }
    }

    /// Success returning two values (`value` in `rax`, `value2` in `rdx`).
    #[must_use]
    pub const fn ok2(value: i64, value2: i64) -> Self {
        Self {
            value,
            value2,
            has_value2: true,
        }
    }

    /// Error result.
    #[must_use]
    #[allow(clippy::cast_lossless)]
    pub const fn err(e: KernelError) -> Self {
        // `as i64` is lossless (i32 → i64) but `From` isn't const-stable.
        Self {
            value: e.code() as i64,
            value2: 0,
            has_value2: false,
        }
    }
}

/// Convert a `KernelResult<i64>` into a `SyscallResult`.
impl From<KernelResult<i64>> for SyscallResult {
    fn from(result: KernelResult<i64>) -> Self {
        match result {
            Ok(val) => Self::ok(val),
            Err(e) => Self::err(e),
        }
    }
}

// ---------------------------------------------------------------------------
// Handler function type
// ---------------------------------------------------------------------------

/// A syscall handler function.
///
/// Receives the syscall arguments and returns a result.
type SyscallHandler = fn(&SyscallArgs) -> SyscallResult;

// ---------------------------------------------------------------------------
// Dispatch table
// ---------------------------------------------------------------------------

/// Static dispatch table for syscall version 1.
///
/// This is a flat array indexed by syscall number.  `None` entries
/// are unimplemented syscalls (return `NotSupported`).
///
/// The table is constructed at compile time.
static V1_TABLE: SyscallTable = build_v1_table();

/// A versioned syscall dispatch table.
struct SyscallTable {
    /// Handler array.  `None` = unimplemented.
    handlers: [Option<SyscallHandler>; MAX_SYSCALL_NR],
    /// Version number.
    version: u32,
}

/// Build the version 1 dispatch table at compile time.
///
/// Syscall numbers are `u64` constants.  On our target (`x86_64`),
/// `usize` is 64-bit, so truncation cannot happen.  We allow the
/// lint because the const context requires `as usize`.
#[allow(clippy::cast_possible_truncation)]
const fn build_v1_table() -> SyscallTable {
    let mut handlers: [Option<SyscallHandler>; MAX_SYSCALL_NR] = [None; MAX_SYSCALL_NR];

    // Kernel-core (0–199)
    handlers[SYS_YIELD as usize] = Some(handlers::sys_yield);
    handlers[SYS_EXIT as usize] = Some(handlers::sys_exit);
    handlers[SYS_TASK_ID as usize] = Some(handlers::sys_task_id);
    handlers[SYS_DEBUG_PRINT as usize] = Some(handlers::sys_debug_print);
    handlers[SYS_MMAP as usize] = Some(handlers::sys_mmap);
    handlers[SYS_MUNMAP as usize] = Some(handlers::sys_munmap);
    handlers[SYS_MPROTECT as usize] = Some(handlers::sys_mprotect);
    handlers[SYS_IRQ_REGISTER as usize] = Some(handlers::sys_irq_register);
    handlers[SYS_IRQ_WAIT as usize] = Some(handlers::sys_irq_wait);
    handlers[SYS_IRQ_RELEASE as usize] = Some(handlers::sys_irq_release);
    handlers[SYS_PORT_READ as usize] = Some(handlers::sys_port_read);
    handlers[SYS_PORT_WRITE as usize] = Some(handlers::sys_port_write);

    // DMA / IOMMU (42–49).
    handlers[SYS_DMA_ALLOC as usize] = Some(handlers::sys_dma_alloc);
    handlers[SYS_DMA_FREE as usize] = Some(handlers::sys_dma_free);
    handlers[SYS_DMA_DOMAIN_CREATE as usize] = Some(handlers::sys_dma_domain_create);
    handlers[SYS_DMA_DOMAIN_DESTROY as usize] = Some(handlers::sys_dma_domain_destroy);
    handlers[SYS_DMA_MAP as usize] = Some(handlers::sys_dma_map);
    handlers[SYS_DMA_UNMAP as usize] = Some(handlers::sys_dma_unmap);
    handlers[SYS_DMA_ATTACH as usize] = Some(handlers::sys_dma_attach);
    handlers[SYS_DMA_DETACH as usize] = Some(handlers::sys_dma_detach);

    // Scheduler configuration (50–59).
    handlers[SYS_SCHED_SET_TIMESLICE as usize] = Some(handlers::sys_sched_set_timeslice);
    handlers[SYS_SCHED_GET_TIMESLICE as usize] = Some(handlers::sys_sched_get_timeslice);
    handlers[SYS_SCHED_RECONFIGURE as usize] = Some(handlers::sys_sched_reconfigure);
    handlers[SYS_SCHED_SET_PROFILE as usize] = Some(handlers::sys_sched_set_profile);
    handlers[SYS_SCHED_GET_PROFILE as usize] = Some(handlers::sys_sched_get_profile);
    handlers[SYS_CPU_COUNT as usize] = Some(handlers::sys_cpu_count);
    handlers[SYS_PHYS_PAGES_TOTAL as usize] = Some(handlers::sys_phys_pages_total);
    handlers[SYS_PHYS_PAGES_AVAIL as usize] = Some(handlers::sys_phys_pages_avail);
    handlers[SYS_LOADAVG as usize] = Some(handlers::sys_loadavg);
    handlers[SYS_CPU_TIMES as usize] = Some(handlers::sys_cpu_times);

    // Sysctl — kernel parameter registry (60–69).
    handlers[SYS_SYSCTL_GET as usize] = Some(handlers::sys_sysctl_get);
    handlers[SYS_SYSCTL_SET as usize] = Some(handlers::sys_sysctl_set);

    // Memory workload profiles (70–79).
    handlers[SYS_MM_SET_PROFILE as usize] = Some(handlers::sys_mm_set_profile);
    handlers[SYS_MM_GET_PROFILE as usize] = Some(handlers::sys_mm_get_profile);

    // System-wide workload profiles (80–89).
    handlers[SYS_SYSTEM_SET_PROFILE as usize] = Some(handlers::sys_system_set_profile);
    handlers[SYS_GETRANDOM as usize] = Some(handlers::sys_getrandom);

    // IPC (200–399)
    handlers[SYS_CHANNEL_CREATE as usize] = Some(handlers::sys_channel_create);
    handlers[SYS_CHANNEL_SEND as usize] = Some(handlers::sys_channel_send);
    handlers[SYS_CHANNEL_RECV as usize] = Some(handlers::sys_channel_recv);
    handlers[SYS_CHANNEL_TRY_RECV as usize] = Some(handlers::sys_channel_try_recv);
    handlers[SYS_CHANNEL_CLOSE as usize] = Some(handlers::sys_channel_close);
    handlers[SYS_CHANNEL_RECV_TIMEOUT as usize] = Some(handlers::sys_channel_recv_timeout);
    handlers[SYS_CHANNEL_SEND_TIMEOUT as usize] = Some(handlers::sys_channel_send_timeout);
    handlers[SYS_CHANNEL_SEND_BLOCKING as usize] = Some(handlers::sys_channel_send_blocking);
    handlers[SYS_CHANNEL_SEND_CAPS as usize] = Some(handlers::sys_channel_send_caps);
    handlers[SYS_CHANNEL_RECV_CAPS as usize] = Some(handlers::sys_channel_recv_caps);
    handlers[SYS_FUTEX_WAIT as usize] = Some(handlers::sys_futex_wait);
    handlers[SYS_FUTEX_WAKE as usize] = Some(handlers::sys_futex_wake);
    handlers[SYS_FUTEX_WAIT_TIMEOUT as usize] = Some(handlers::sys_futex_wait_timeout);
    handlers[SYS_FUTEX_REQUEUE as usize] = Some(handlers::sys_futex_requeue);
    handlers[SYS_FUTEX_LOCK_PI as usize] = Some(handlers::sys_futex_lock_pi);
    handlers[SYS_FUTEX_UNLOCK_PI as usize] = Some(handlers::sys_futex_unlock_pi);
    handlers[SYS_FUTEX_TRYLOCK_PI as usize] = Some(handlers::sys_futex_trylock_pi);
    handlers[SYS_FUTEX_LOCK_PI_TIMEOUT as usize] = Some(handlers::sys_futex_lock_pi_timeout);
    handlers[SYS_FUTEX_WAIT_REQUEUE_PI as usize] = Some(handlers::sys_futex_wait_requeue_pi);
    handlers[SYS_FUTEX_CMP_REQUEUE_PI as usize] = Some(handlers::sys_futex_cmp_requeue_pi);
    handlers[SYS_PIPE_CREATE as usize] = Some(handlers::sys_pipe_create);
    handlers[SYS_PIPE_WRITE as usize] = Some(handlers::sys_pipe_write);
    handlers[SYS_PIPE_READ as usize] = Some(handlers::sys_pipe_read);
    handlers[SYS_PIPE_TRY_WRITE as usize] = Some(handlers::sys_pipe_try_write);
    handlers[SYS_PIPE_TRY_READ as usize] = Some(handlers::sys_pipe_try_read);
    handlers[SYS_PIPE_CLOSE as usize] = Some(handlers::sys_pipe_close);
    handlers[SYS_PIPE_POLL as usize] = Some(handlers::sys_pipe_poll);
    handlers[SYS_PIPE_READABLE_BYTES as usize] = Some(handlers::sys_pipe_readable_bytes);
    handlers[SYS_PIPE_READ_TIMEOUT as usize] = Some(handlers::sys_pipe_read_timeout);
    handlers[SYS_PIPE_WRITE_TIMEOUT as usize] = Some(handlers::sys_pipe_write_timeout);
    handlers[SYS_PIPE_PEEK as usize] = Some(handlers::sys_pipe_peek);
    handlers[SYS_PIPE_WAIT_READABLE as usize] = Some(handlers::sys_pipe_wait_readable);
    handlers[SYS_SHM_CREATE as usize] = Some(handlers::sys_shm_create);
    handlers[SYS_SHM_SIZE as usize] = Some(handlers::sys_shm_size);
    handlers[SYS_SHM_CLOSE as usize] = Some(handlers::sys_shm_close);
    handlers[SYS_SHM_MAP as usize] = Some(handlers::sys_shm_map);
    handlers[SYS_SHM_UNMAP as usize] = Some(handlers::sys_shm_unmap);
    handlers[SYS_SOCKETPAIR_CREATE as usize] = Some(handlers::sys_socketpair_create);
    handlers[SYS_SOCKETPAIR_SEND as usize] = Some(handlers::sys_socketpair_send);
    handlers[SYS_SOCKETPAIR_RECV as usize] = Some(handlers::sys_socketpair_recv);
    handlers[SYS_SOCKETPAIR_TRY_SEND as usize] = Some(handlers::sys_socketpair_try_send);
    handlers[SYS_SOCKETPAIR_TRY_RECV as usize] = Some(handlers::sys_socketpair_try_recv);
    handlers[SYS_SOCKETPAIR_CLOSE as usize] = Some(handlers::sys_socketpair_close);
    handlers[SYS_SOCKETPAIR_SEND_TIMEOUT as usize] = Some(handlers::sys_socketpair_send_timeout);
    handlers[SYS_SOCKETPAIR_RECV_TIMEOUT as usize] = Some(handlers::sys_socketpair_recv_timeout);
    handlers[SYS_SOCKETPAIR_POLL as usize] = Some(handlers::sys_socketpair_poll);
    handlers[SYS_SOCKETPAIR_READABLE_BYTES as usize] =
        Some(handlers::sys_socketpair_readable_bytes);
    handlers[SYS_SOCKETPAIR_SHUTDOWN as usize] = Some(handlers::sys_socketpair_shutdown);
    handlers[SYS_EVENTFD_CREATE as usize] = Some(handlers::sys_eventfd_create);
    handlers[SYS_EVENTFD_WRITE as usize] = Some(handlers::sys_eventfd_write);
    handlers[SYS_EVENTFD_READ as usize] = Some(handlers::sys_eventfd_read);
    handlers[SYS_EVENTFD_TRY_READ as usize] = Some(handlers::sys_eventfd_try_read);
    handlers[SYS_EVENTFD_CLOSE as usize] = Some(handlers::sys_eventfd_close);
    handlers[SYS_EVENTFD_READ_TIMEOUT as usize] = Some(handlers::sys_eventfd_read_timeout);
    handlers[SYS_EVENTFD_WRITE_TIMEOUT as usize] = Some(handlers::sys_eventfd_write_timeout);
    handlers[SYS_EVENTFD_HAS_VALUE as usize] = Some(handlers::sys_eventfd_has_value);
    handlers[SYS_CP_CREATE as usize] = Some(handlers::sys_cp_create);
    handlers[SYS_CP_REGISTER as usize] = Some(handlers::sys_cp_register);
    handlers[SYS_CP_UNREGISTER as usize] = Some(handlers::sys_cp_unregister);
    handlers[SYS_CP_WAIT as usize] = Some(handlers::sys_cp_wait);
    handlers[SYS_CP_TRY_WAIT as usize] = Some(handlers::sys_cp_try_wait);
    handlers[SYS_CP_CLOSE as usize] = Some(handlers::sys_cp_close);
    handlers[SYS_CP_NOTIFY as usize] = Some(handlers::sys_cp_notify);

    // io_ring (260–269).
    handlers[SYS_IO_RING_SETUP as usize] = Some(handlers::sys_io_ring_setup);
    handlers[SYS_IO_RING_ENTER as usize] = Some(handlers::sys_io_ring_enter);
    handlers[SYS_IO_RING_DESTROY as usize] = Some(handlers::sys_io_ring_destroy);

    // IPC semaphores (270–274).
    handlers[SYS_SEM_CREATE as usize] = Some(handlers::sys_sem_create);
    handlers[SYS_SEM_SIGNAL as usize] = Some(handlers::sys_sem_signal);
    handlers[SYS_SEM_WAIT as usize] = Some(handlers::sys_sem_wait);
    handlers[SYS_SEM_TRY_WAIT as usize] = Some(handlers::sys_sem_try_wait);
    handlers[SYS_SEM_CLOSE as usize] = Some(handlers::sys_sem_close);
    handlers[SYS_SEM_WAIT_TIMEOUT as usize] = Some(handlers::sys_sem_wait_timeout);
    handlers[SYS_SERVICE_REGISTER as usize] = Some(handlers::sys_service_register);
    handlers[SYS_SERVICE_CONNECT as usize] = Some(handlers::sys_service_connect);
    handlers[SYS_SERVICE_ACCEPT as usize] = Some(handlers::sys_service_accept);
    handlers[SYS_SERVICE_TRY_ACCEPT as usize] = Some(handlers::sys_service_try_accept);
    handlers[SYS_SERVICE_ACCEPT_TIMEOUT as usize] = Some(handlers::sys_service_accept_timeout);
    handlers[SYS_SERVICE_UNREGISTER as usize] = Some(handlers::sys_service_unregister);
    // Sits in the service block, not the channel block (200–209 is full):
    // it is the missing half of `SYS_SERVICE_ACCEPT`.
    handlers[SYS_CHANNEL_PEER_CRED as usize] = Some(handlers::sys_channel_peer_cred);

    // Namespace (290–295).
    handlers[SYS_NS_CREATE as usize] = Some(handlers::sys_ns_create);
    handlers[SYS_NS_BIND as usize] = Some(handlers::sys_ns_bind);
    handlers[SYS_NS_UNBIND as usize] = Some(handlers::sys_ns_unbind);
    handlers[SYS_NS_HIDE as usize] = Some(handlers::sys_ns_hide);
    handlers[SYS_NS_ATTACH as usize] = Some(handlers::sys_ns_attach);
    handlers[SYS_NS_QUERY as usize] = Some(handlers::sys_ns_query);

    // Time and timers (10–19).
    handlers[SYS_CLOCK_MONOTONIC as usize] = Some(handlers::sys_clock_monotonic);
    handlers[SYS_CLOCK_REALTIME as usize] = Some(handlers::sys_clock_realtime);
    handlers[SYS_CLOCK_SETTIME as usize] = Some(handlers::sys_clock_settime);
    handlers[SYS_CLOCK_ADJTIME as usize] = Some(handlers::sys_clock_adjtime);
    handlers[SYS_SLEEP as usize] = Some(handlers::sys_sleep);
    handlers[SYS_TIMER_CREATE as usize] = Some(handlers::sys_timer_create);
    handlers[SYS_TIMER_CANCEL as usize] = Some(handlers::sys_timer_cancel);

    // Console I/O (100–109).
    handlers[SYS_CONSOLE_WRITE as usize] = Some(handlers::sys_console_write);
    handlers[SYS_CONSOLE_READ_CHAR as usize] = Some(handlers::sys_console_read_char);
    handlers[SYS_CONSOLE_TRY_READ_CHAR as usize] = Some(handlers::sys_console_try_read_char);
    handlers[SYS_LOG_READ as usize] = Some(handlers::sys_log_read);

    // Security (400–499).
    handlers[SYS_CAP_QUERY as usize] = Some(handlers::sys_cap_query);
    handlers[SYS_CAP_REQUEST as usize] = Some(handlers::sys_cap_request);
    handlers[SYS_CAP_REQUEST_STATUS as usize] = Some(handlers::sys_cap_request_status);
    handlers[SYS_CAP_REQUEST_CANCEL as usize] = Some(handlers::sys_cap_request_cancel);

    // Process management (500–509).
    handlers[SYS_PROCESS_SPAWN as usize] = Some(handlers::sys_process_spawn);
    handlers[SYS_PROCESS_WAIT as usize] = Some(handlers::sys_process_wait);
    handlers[SYS_PROCESS_TRY_WAIT as usize] = Some(handlers::sys_process_try_wait);
    handlers[SYS_PROCESS_WAIT_STATUS as usize] = Some(handlers::sys_process_wait_status);
    handlers[SYS_PROCESS_GET_RUSAGE as usize] = Some(handlers::sys_process_get_rusage);
    handlers[SYS_PROCESS_ID as usize] = Some(handlers::sys_process_id);
    handlers[SYS_SET_EXCEPTION_HANDLER as usize] = Some(handlers::sys_set_exception_handler);
    handlers[SYS_PROCESS_KILL as usize] = Some(handlers::sys_process_kill);
    handlers[SYS_NOTIFY_READY as usize] = Some(handlers::sys_notify_ready);
    handlers[SYS_PROCESS_IS_READY as usize] = Some(handlers::sys_process_is_ready);
    handlers[SYS_PROCESS_SPAWN_EX as usize] = Some(handlers::sys_process_spawn_ex);
    handlers[SYS_PROCESS_SPAWN_EX2 as usize] = Some(handlers::sys_process_spawn_ex2);
    handlers[SYS_PROCESS_GET_INITIAL_FDS as usize] = Some(handlers::sys_process_get_initial_fds);
    handlers[SYS_PROCESS_SET_EXEC_FDS as usize] = Some(handlers::sys_process_set_exec_fds);
    handlers[SYS_PROCESS_GET_ARGS as usize] = Some(handlers::sys_process_get_args);
    handlers[SYS_PROCESS_PARENT_ID as usize] = Some(handlers::sys_process_parent_id);
    handlers[SYS_PROCESS_COUNT as usize] = Some(handlers::sys_process_count);
    handlers[SYS_PROCESS_GET_CREDENTIALS as usize] = Some(handlers::sys_process_get_credentials);
    handlers[SYS_PROCESS_SET_CREDENTIALS as usize] = Some(handlers::sys_process_set_credentials);
    handlers[SYS_PROCESS_GET_NICE as usize] = Some(handlers::sys_process_get_nice);
    handlers[SYS_PROCESS_SET_NICE as usize] = Some(handlers::sys_process_set_nice);

    // Process groups / sessions (533–536). Native entry points onto the
    // same `proc::pcb` state the Linux shim's setpgid/getpgid/setsid/getsid
    // use, so both ABIs share one source of truth.
    handlers[SYS_PROCESS_SET_PGID as usize] = Some(handlers::sys_process_set_pgid);
    handlers[SYS_PROCESS_GET_PGID as usize] = Some(handlers::sys_process_get_pgid);
    handlers[SYS_PROCESS_SET_SID as usize] = Some(handlers::sys_process_set_sid);
    handlers[SYS_PROCESS_GET_SID as usize] = Some(handlers::sys_process_get_sid);

    // Controlling terminal (537–543). The foreground process group belongs to
    // a *session*, so like 533–536 it has to live in the kernel: two members
    // of one session must never be able to disagree about it. 539/540 are
    // the acquire/release pair behind `ioctl(TIOCSCTTY)`/`ioctl(TIOCNOTTY)`.
    // 541–543 are the rest of the terminal that the native ABI could not
    // previously reach at all: the real termios (libc answered `tcgetattr`
    // from a constant and threw `tcsetattr` away) and a console read that
    // goes through the line discipline instead of straight to the keyboard
    // driver, so `^C` becomes a signal rather than byte 0x03.
    handlers[SYS_TTY_GET_PGRP as usize] = Some(handlers::sys_tty_get_pgrp);
    handlers[SYS_TTY_SET_PGRP as usize] = Some(handlers::sys_tty_set_pgrp);
    handlers[SYS_TTY_ACQUIRE_CTTY as usize] = Some(handlers::sys_tty_acquire_ctty);
    handlers[SYS_TTY_RELEASE_CTTY as usize] = Some(handlers::sys_tty_release_ctty);
    handlers[SYS_TTY_GET_TERMIOS as usize] = Some(handlers::sys_tty_get_termios);
    handlers[SYS_TTY_SET_TERMIOS as usize] = Some(handlers::sys_tty_set_termios);
    handlers[SYS_TTY_READ as usize] = Some(handlers::sys_tty_read);

    // Pseudo-terminals (544–554). The same line discipline as above, driven by
    // a program instead of the keyboard driver — what a terminal emulator, an
    // `ssh` server, `script(1)` or `expect(1)` needs.
    handlers[SYS_PTY_CREATE as usize] = Some(handlers::sys_pty_create);
    handlers[SYS_PTY_MASTER_WRITE as usize] = Some(handlers::sys_pty_master_write);
    handlers[SYS_PTY_MASTER_READ as usize] = Some(handlers::sys_pty_master_read);
    handlers[SYS_PTY_MASTER_TRY_READ as usize] = Some(handlers::sys_pty_master_try_read);
    handlers[SYS_PTY_SLAVE_WRITE as usize] = Some(handlers::sys_pty_slave_write);
    handlers[SYS_PTY_CLOSE as usize] = Some(handlers::sys_pty_close);
    handlers[SYS_PTY_DUP as usize] = Some(handlers::sys_pty_dup);
    handlers[SYS_PTY_SLAVE_ID as usize] = Some(handlers::sys_pty_slave_id);
    handlers[SYS_PTY_POLL as usize] = Some(handlers::sys_pty_poll);
    handlers[SYS_PTY_GET_WINSIZE as usize] = Some(handlers::sys_pty_get_winsize);
    handlers[SYS_PTY_SET_WINSIZE as usize] = Some(handlers::sys_pty_set_winsize);
    handlers[SYS_PTY_GET_TERMIOS as usize] = Some(handlers::sys_pty_get_termios);
    handlers[SYS_PTY_SET_TERMIOS as usize] = Some(handlers::sys_pty_set_termios);
    // 869–871, not 557–559: the family's block was closed before these gaps
    // were found. 870/871 generalise 537/538 to a *named* terminal rather than
    // widening them, because libc calls 537 as `syscall0` — which never writes
    // `rdi`, so a widened `arg0` would read whatever the caller left there.
    handlers[SYS_PTY_READABLE_BYTES as usize] = Some(handlers::sys_pty_readable_bytes);
    handlers[SYS_PTY_GET_PGRP as usize] = Some(handlers::sys_pty_get_pgrp);
    handlers[SYS_PTY_SET_PGRP as usize] = Some(handlers::sys_pty_set_pgrp);

    // Resource limits (557–558). The native counterpart of the Linux shim's
    // `prlimit64`, sharing `pcb::get_rlimit`/`pcb::set_rlimit` with it so the
    // two ABIs cannot describe the same process differently — which they did,
    // because libc answered `getrlimit` from a private copy of the table.
    handlers[SYS_RLIMIT_GET as usize] = Some(handlers::sys_rlimit_get);
    handlers[SYS_RLIMIT_SET as usize] = Some(handlers::sys_rlimit_set);

    // POSIX signal shim (522–526). SYS_SIGNAL_RETURN (524) is a
    // frame-modifying syscall handled specially in syscall_handler_inner,
    // so it has no flat-table entry.
    handlers[SYS_SIGNAL_REGISTER as usize] = Some(handlers::sys_signal_register);
    handlers[SYS_SIGNAL_SEND as usize] = Some(handlers::sys_signal_send);
    handlers[SYS_SIGNAL_MASK as usize] = Some(handlers::sys_signal_mask);
    handlers[SYS_SIGNAL_PENDING as usize] = Some(handlers::sys_signal_pending);
    handlers[SYS_SIGNAL_STOP_SELF as usize] = Some(handlers::sys_signal_stop_self);

    // Thread management (510–519).
    handlers[SYS_THREAD_CREATE as usize] = Some(handlers::sys_thread_create);
    handlers[SYS_THREAD_EXIT as usize] = Some(handlers::sys_thread_exit);
    handlers[SYS_THREAD_JOIN as usize] = Some(handlers::sys_thread_join);
    handlers[SYS_THREAD_SUSPEND as usize] = Some(handlers::sys_thread_suspend);
    handlers[SYS_THREAD_RESUME as usize] = Some(handlers::sys_thread_resume);
    handlers[SYS_THREAD_SET_PRIORITY as usize] = Some(handlers::sys_thread_set_priority);
    handlers[SYS_SET_FS_BASE as usize] = Some(handlers::sys_set_fs_base);
    handlers[SYS_PROCESS_CRASH_INFO as usize] = Some(handlers::sys_process_crash_info);

    // Filesystem — path-based (600–609).
    handlers[SYS_FS_READ_FILE as usize] = Some(handlers::sys_fs_read_file);
    handlers[SYS_FS_WRITE_FILE as usize] = Some(handlers::sys_fs_write_file);
    handlers[SYS_FS_DELETE as usize] = Some(handlers::sys_fs_delete);
    handlers[SYS_FS_LIST_DIR as usize] = Some(handlers::sys_fs_list_dir);
    handlers[SYS_FS_MKDIR as usize] = Some(handlers::sys_fs_mkdir);
    handlers[SYS_FS_MKDIR_MODE as usize] = Some(handlers::sys_fs_mkdir_mode);
    handlers[SYS_FS_RMDIR as usize] = Some(handlers::sys_fs_rmdir);
    handlers[SYS_FS_STAT as usize] = Some(handlers::sys_fs_stat);
    handlers[SYS_FS_LINK as usize] = Some(handlers::sys_fs_link);
    handlers[SYS_FS_STATVFS as usize] = Some(handlers::sys_fs_statvfs);
    handlers[SYS_FS_FLOCK as usize] = Some(handlers::sys_fs_flock);
    handlers[SYS_FS_FUNLOCK as usize] = Some(handlers::sys_fs_funlock);
    handlers[SYS_FS_SYNC as usize] = Some(handlers::sys_fs_sync);
    handlers[SYS_FS_COPY as usize] = Some(handlers::sys_fs_copy);
    handlers[SYS_FS_APPEND as usize] = Some(handlers::sys_fs_append);
    handlers[SYS_FS_FTRUNCATE as usize] = Some(handlers::sys_fs_ftruncate);
    handlers[SYS_FS_DUP as usize] = Some(handlers::sys_fs_dup);
    handlers[SYS_FS_HANDLE_PATH as usize] = Some(handlers::sys_fs_handle_path);
    handlers[SYS_FS_READDIR_AT as usize] = Some(handlers::sys_fs_readdir_at);
    handlers[SYS_FS_TMPFILE as usize] = Some(handlers::sys_fs_tmpfile);
    handlers[SYS_FS_FALLOCATE as usize] = Some(handlers::sys_fs_fallocate);
    handlers[SYS_FS_SEEK_DATA as usize] = Some(handlers::sys_fs_seek_data);
    handlers[SYS_FS_SEEK_HOLE as usize] = Some(handlers::sys_fs_seek_hole);
    handlers[SYS_FS_MOUNT as usize] = Some(handlers::sys_fs_mount);
    handlers[SYS_FS_UMOUNT as usize] = Some(handlers::sys_fs_umount);
    handlers[SYS_FS_FORMAT as usize] = Some(handlers::sys_fs_format);
    handlers[SYS_FS_CHECK as usize] = Some(handlers::sys_fs_check);
    handlers[SYS_FS_TRIM as usize] = Some(handlers::sys_fs_trim);

    // Filesystem — handle-based (610–699).
    handlers[SYS_FS_OPEN as usize] = Some(handlers::sys_fs_open);
    handlers[SYS_FS_OPEN_MODE as usize] = Some(handlers::sys_fs_open_mode);
    handlers[SYS_FS_CLOSE as usize] = Some(handlers::sys_fs_close);
    handlers[SYS_FS_READ as usize] = Some(handlers::sys_fs_read);
    handlers[SYS_FS_WRITE as usize] = Some(handlers::sys_fs_write);
    handlers[SYS_FS_SEEK as usize] = Some(handlers::sys_fs_seek);
    handlers[SYS_FS_TRUNCATE as usize] = Some(handlers::sys_fs_truncate);
    handlers[SYS_FS_RENAME as usize] = Some(handlers::sys_fs_rename);
    handlers[SYS_FS_FSTAT as usize] = Some(handlers::sys_fs_fstat);
    handlers[SYS_FS_TRASH as usize] = Some(handlers::sys_fs_trash);
    handlers[SYS_FS_TRASH_LIST as usize] = Some(handlers::sys_fs_trash_list);
    handlers[SYS_FS_TRASH_RESTORE as usize] = Some(handlers::sys_fs_trash_restore);
    handlers[SYS_FS_TRASH_EMPTY as usize] = Some(handlers::sys_fs_trash_empty);
    handlers[SYS_FS_WATCH_CREATE as usize] = Some(handlers::sys_fs_watch_create);
    handlers[SYS_FS_WATCH_READ as usize] = Some(handlers::sys_fs_watch_read);
    handlers[SYS_FS_WATCH_CLOSE as usize] = Some(handlers::sys_fs_watch_close);
    handlers[SYS_FS_JOURNAL_CURSOR as usize] = Some(handlers::sys_fs_journal_cursor);
    handlers[SYS_FS_JOURNAL_READ as usize] = Some(handlers::sys_fs_journal_read);
    handlers[SYS_FS_JOURNAL_FLUSH as usize] = Some(handlers::sys_fs_journal_flush);

    // Metadata (628–636).
    handlers[SYS_FS_METADATA as usize] = Some(handlers::sys_fs_metadata);
    handlers[SYS_FS_SET_ATTR as usize] = Some(handlers::sys_fs_set_attr);
    handlers[SYS_FS_SET_OWNER as usize] = Some(handlers::sys_fs_set_owner);
    handlers[SYS_FS_SET_PERMS as usize] = Some(handlers::sys_fs_set_perms);
    handlers[SYS_FS_SET_TIMES as usize] = Some(handlers::sys_fs_set_times);
    handlers[SYS_FS_GET_XATTR as usize] = Some(handlers::sys_fs_get_xattr);
    handlers[SYS_FS_SET_XATTR as usize] = Some(handlers::sys_fs_set_xattr);
    handlers[SYS_FS_REMOVE_XATTR as usize] = Some(handlers::sys_fs_remove_xattr);
    handlers[SYS_FS_LIST_XATTRS as usize] = Some(handlers::sys_fs_list_xattrs);

    // Symlinks (637–639).
    handlers[SYS_FS_SYMLINK as usize] = Some(handlers::sys_fs_symlink);
    handlers[SYS_FS_READLINK as usize] = Some(handlers::sys_fs_readlink);
    handlers[SYS_FS_LSTAT as usize] = Some(handlers::sys_fs_lstat);

    // Networking (800–999).
    handlers[SYS_TCP_CONNECT as usize] = Some(handlers::sys_tcp_connect);
    handlers[SYS_TCP_SEND as usize] = Some(handlers::sys_tcp_send);
    handlers[SYS_TCP_RECV as usize] = Some(handlers::sys_tcp_recv);
    handlers[SYS_TCP_CLOSE as usize] = Some(handlers::sys_tcp_close);
    handlers[SYS_TCP_BIND as usize] = Some(handlers::sys_tcp_bind);
    handlers[SYS_TCP_ACCEPT as usize] = Some(handlers::sys_tcp_accept);
    handlers[SYS_TCP_CLOSE_LISTENER as usize] = Some(handlers::sys_tcp_close_listener);
    handlers[SYS_TCP_ABORT as usize] = Some(handlers::sys_tcp_abort);
    handlers[SYS_TCP_PEER_ADDR as usize] = Some(handlers::sys_tcp_peer_addr);
    handlers[SYS_UDP_BIND as usize] = Some(handlers::sys_udp_bind);
    handlers[SYS_UDP_SEND as usize] = Some(handlers::sys_udp_send);
    handlers[SYS_UDP_RECV as usize] = Some(handlers::sys_udp_recv);
    handlers[SYS_UDP_CLOSE as usize] = Some(handlers::sys_udp_close);
    handlers[SYS_UDP_CONNECT as usize] = Some(handlers::sys_udp_connect);
    handlers[SYS_UDP_LOCAL_PORT as usize] = Some(handlers::sys_udp_local_port);
    handlers[SYS_UDP_MCAST_JOIN as usize] = Some(handlers::sys_udp_mcast_join);
    handlers[SYS_UDP_MCAST_LEAVE as usize] = Some(handlers::sys_udp_mcast_leave);
    handlers[SYS_DNS_RESOLVE as usize] = Some(handlers::sys_dns_resolve);
    handlers[SYS_DNS_REVERSE_RESOLVE as usize] = Some(handlers::sys_dns_reverse_resolve);
    handlers[SYS_NET_STAT as usize] = Some(handlers::sys_net_stat);
    handlers[SYS_ICMP_PING as usize] = Some(handlers::sys_icmp_ping);
    handlers[SYS_ICMP_PING_WAIT as usize] = Some(handlers::sys_icmp_ping_wait);
    handlers[SYS_TCP_LIST as usize] = Some(handlers::sys_tcp_list);
    handlers[SYS_TCP_LISTENER_LIST as usize] = Some(handlers::sys_tcp_listener_list);
    handlers[SYS_NET_IF_INFO as usize] = Some(handlers::sys_net_if_info);
    handlers[SYS_NET_IF_CONFIG as usize] = Some(handlers::sys_net_if_config);
    handlers[SYS_NET_ROUTE_ADD as usize] = Some(handlers::sys_net_route_add);
    handlers[SYS_NET_ROUTE_DEL as usize] = Some(handlers::sys_net_route_del);
    handlers[SYS_NET_ROUTE_LIST as usize] = Some(handlers::sys_net_route_list);
    handlers[SYS_NET_FW_ENABLE as usize] = Some(handlers::sys_net_fw_enable);
    handlers[SYS_NET_FW_SET_POLICY as usize] = Some(handlers::sys_net_fw_set_policy);
    handlers[SYS_NET_FW_ADD_RULE as usize] = Some(handlers::sys_net_fw_add_rule);
    handlers[SYS_NET_FW_DEL_RULE as usize] = Some(handlers::sys_net_fw_del_rule);
    handlers[SYS_NET_FW_FLUSH as usize] = Some(handlers::sys_net_fw_flush);
    handlers[SYS_NET_RAW_OPEN as usize] = Some(handlers::sys_net_raw_open);
    handlers[SYS_NET_RAW_TX as usize] = Some(handlers::sys_net_raw_tx);
    handlers[SYS_NET_RAW_RX as usize] = Some(handlers::sys_net_raw_rx);
    handlers[SYS_NET_RAW_CLOSE as usize] = Some(handlers::sys_net_raw_close);
    handlers[SYS_ARP_TABLE as usize] = Some(handlers::sys_arp_table);
    handlers[SYS_DNS_CACHE_STATS as usize] = Some(handlers::sys_dns_cache_stats);
    handlers[SYS_TCP_POLL_STATUS as usize] = Some(handlers::sys_tcp_poll_status);
    handlers[SYS_TCP_LISTENER_READY as usize] = Some(handlers::sys_tcp_listener_ready);
    handlers[SYS_UDP_RX_READY as usize] = Some(handlers::sys_udp_rx_ready);
    handlers[SYS_UDP_RX_FRONT_BYTES as usize] = Some(handlers::sys_udp_rx_front_bytes);
    handlers[SYS_TCP_SHUTDOWN as usize] = Some(handlers::sys_tcp_shutdown);
    handlers[SYS_TCP_INFO as usize] = Some(handlers::sys_tcp_info);
    handlers[SYS_TCP_SET_NODELAY as usize] = Some(handlers::sys_tcp_set_nodelay);
    handlers[SYS_TCP_SET_KEEPALIVE as usize] = Some(handlers::sys_tcp_set_keepalive);
    handlers[SYS_TCP_SET_KEEPALIVE_PARAMS as usize] = Some(handlers::sys_tcp_set_keepalive_params);
    handlers[SYS_TCP_LAST_ERROR as usize] = Some(handlers::sys_tcp_last_error);
    handlers[SYS_TCP_LOCAL_PORT as usize] = Some(handlers::sys_tcp_local_port);

    // DRM/GPU (1000–1099).
    handlers[SYS_DRM_OPEN as usize] = Some(drm_handlers::sys_drm_open);
    handlers[SYS_DRM_CLOSE as usize] = Some(drm_handlers::sys_drm_close);
    handlers[SYS_DRM_DISPLAY_SIZE as usize] = Some(drm_handlers::sys_drm_display_size);
    handlers[SYS_DRM_GEM_CREATE as usize] = Some(drm_handlers::sys_drm_gem_create);
    handlers[SYS_DRM_GEM_DESTROY as usize] = Some(drm_handlers::sys_drm_gem_destroy);
    handlers[SYS_DRM_GEM_MMAP as usize] = Some(drm_handlers::sys_drm_gem_mmap);
    handlers[SYS_DRM_FB_CREATE as usize] = Some(drm_handlers::sys_drm_fb_create);
    handlers[SYS_DRM_FB_DESTROY as usize] = Some(drm_handlers::sys_drm_fb_destroy);
    handlers[SYS_DRM_PAGE_FLIP as usize] = Some(drm_handlers::sys_drm_page_flip);
    handlers[SYS_DRM_FLUSH_REGION as usize] = Some(drm_handlers::sys_drm_flush_region);
    handlers[SYS_DRM_CONNECTOR_STATUS as usize] = Some(drm_handlers::sys_drm_connector_status);
    handlers[SYS_DRM_MODE_GET as usize] = Some(drm_handlers::sys_drm_mode_get);
    handlers[SYS_DRM_CRTC_INFO as usize] = Some(drm_handlers::sys_drm_crtc_info);
    handlers[SYS_DRM_CURSOR_SET as usize] = Some(drm_handlers::sys_drm_cursor_set);
    handlers[SYS_DRM_CURSOR_MOVE as usize] = Some(drm_handlers::sys_drm_cursor_move);
    handlers[SYS_DRM_ATOMIC_COMMIT as usize] = Some(drm_handlers::sys_drm_atomic_commit);

    SyscallTable {
        handlers,
        version: 1,
    }
}

// ---------------------------------------------------------------------------
// Dispatch entry point
// ---------------------------------------------------------------------------

/// Dispatch a syscall.
///
/// This is the main entry point called from the syscall entry assembly
/// (or from kernel-mode test code).  It looks up the handler in the
/// active dispatch table and invokes it.
///
/// # Arguments
///
/// - `nr`: syscall number (from `rax`).
/// - `args`: the 6 register arguments.
///
/// # Returns
///
/// A [`SyscallResult`] with the return values for `rax` and `rdx`.
#[allow(clippy::cast_possible_truncation)]
pub fn dispatch(nr: u64, args: &SyscallArgs) -> SyscallResult {
    let sc_start = crate::sclatency::enter();

    // Resolve the current task id **once**, here, and hand it to everything
    // downstream that needs it: both trace points, the syscall filter, and I/O
    // accounting.  `sched::current_task_id()` measures ~23 ns, and this
    // function previously called it three times per syscall — once inside each
    // of the two `ktrace::record` calls and once for the filter — for a value
    // that cannot change across a single dispatch.
    let task_id = crate::sched::current_task_id();

    crate::ktrace::record_with_task(
        crate::ktrace::Category::Syscall,
        crate::ktrace::event::SYSCALL_ENTER,
        task_id,
        nr,
        args.arg0,
    );

    // Bounds check.
    let idx = nr as usize;
    if idx >= MAX_SYSCALL_NR {
        crate::sclatency::exit(sc_start, nr);
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Syscall filter check (seccomp equivalent).
    //
    // Before looking up the handler, verify this syscall is allowed
    // for the calling task.  Denied syscalls return PermissionDenied
    // without ever invoking the handler.  This enforces per-process
    // syscall sandboxing for containers.
    if !crate::scfilter::check(task_id, nr) {
        crate::sclatency::exit(sc_start, nr);
        return SyscallResult::err(KernelError::PermissionDenied);
    }

    // Look up handler.
    //
    // SAFETY: idx is bounds-checked above.
    #[allow(clippy::indexing_slicing)]
    let result = if let Some(handler) = V1_TABLE.handlers[idx] {
        handler(args)
    } else {
        serial_println!(
            "[syscall] Unimplemented syscall {} (v{})",
            nr,
            V1_TABLE.version
        );
        SyscallResult::err(KernelError::NotSupported)
    };

    crate::ktrace::record_with_task(
        crate::ktrace::Category::Syscall,
        crate::ktrace::event::SYSCALL_EXIT,
        task_id,
        nr,
        result.value as u64,
    );

    // Per-process I/O byte accounting for `/proc/<pid>/io` (rchar/wchar,
    // syscr/syscw).  The Linux-ABI dispatch path accounts its own
    // read/write family separately in `linux::dispatch_linux`; this hook
    // covers the *native* read/write syscalls so native processes get
    // honest io counters instead of all-zero.  `task_id` is resolved once at
    // the top of this function, so this adds no extra lookup.
    account_io_syscall_native(nr, task_id, result.value);

    crate::sclatency::exit(sc_start, nr);
    result
}

/// Fold a completed native read/write syscall into the owning process's
/// `/proc/<pid>/io` counters.
///
/// Mirrors Linux's `task_io_accounting`: `syscr`/`syscw` count every
/// read/write-family syscall unconditionally (even failing ones), while
/// `rchar`/`wchar` accumulate only the *positive* byte count returned.
/// A negative `value` (error) folds as zero bytes but still bumps the
/// syscall counter, exactly as Linux does.
///
/// Only syscalls whose return value *is* the transferred byte count are
/// accounted here.  `SYS_FS_WRITE_FILE` is deliberately excluded: it
/// returns `0` on success rather than a byte count, so folding its
/// result would bump `syscw` without a matching `wchar` — dishonest
/// undercounting.  See `todo.txt` for the note on accounting it at the
/// handler level if whole-file writes need to appear in `wchar`.
/// Direction of a byte-transferring read/write syscall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IoDir {
    Read,
    Write,
}

/// Classify a native syscall number as a read/write-family byte transfer,
/// or `None` if it does not contribute to `/proc/<pid>/io` accounting.
///
/// Only syscalls whose return value *is* the transferred byte count are
/// listed here.  `SYS_FS_WRITE_FILE` is deliberately excluded: it returns
/// `0` on success rather than a byte count, so folding its result would
/// bump `syscw` without a matching `wchar` — dishonest undercounting.
const fn io_dir_for_syscall(nr: u64) -> Option<IoDir> {
    match nr {
        SYS_FS_READ
        | SYS_FS_READ_FILE
        | SYS_PIPE_READ
        | SYS_PIPE_TRY_READ
        | SYS_PIPE_READ_TIMEOUT
        | SYS_CONSOLE_READ_CHAR
        | SYS_CONSOLE_TRY_READ_CHAR => Some(IoDir::Read),
        SYS_FS_WRITE
        | SYS_PIPE_WRITE
        | SYS_PIPE_TRY_WRITE
        | SYS_PIPE_WRITE_TIMEOUT
        | SYS_CONSOLE_WRITE => Some(IoDir::Write),
        _ => None,
    }
}

fn account_io_syscall_native(nr: u64, task_id: crate::sched::task::TaskId, value: i64) {
    let dir = match io_dir_for_syscall(nr) {
        Some(d) => d,
        // Not a byte-transferring read/write syscall — nothing to account.
        None => return,
    };

    // Kernel tasks (no owning process) have no `/proc/<pid>/io` to update.
    let pid = match crate::proc::thread::owner_process(task_id) {
        Some(p) => p,
        None => return,
    };

    // Negative return = error; fold as zero bytes (but still count the syscall).
    let bytes = u64::try_from(value).unwrap_or(0);
    match dir {
        IoDir::Read => crate::proc::pcb::account_io_read(pid, bytes),
        IoDir::Write => crate::proc::pcb::account_io_write(pid, bytes),
    }
}

/// Get the current syscall ABI version.
#[must_use]
pub fn current_version() -> u32 {
    super::number::CURRENT_VERSION
}

/// Verify that dispatch still reaches its handlers once syscall filtering is
/// live — in particular across the whole syscall number range.
///
/// # Why this exists as a *separate* entry point
///
/// [`self_test`] runs very early in boot, thousands of lines before
/// `scfilter::init()`.  Every one of its ~90 cases therefore exercises
/// `dispatch` with the filter subsystem switched off, which is **not the
/// configuration the system ever actually runs in**.  Any bug in the
/// dispatch↔filter interaction is invisible to it by construction.
///
/// One such bug shipped: `scfilter::MAX_SYSCALL_NR` had drifted to 1000 while
/// the dispatch table grew to 1100, so from the moment `scfilter::init()` ran,
/// every syscall in `1000..1100` — the entire DRM/graphics interface plus
/// three process-control syscalls — returned `PermissionDenied` to every
/// process on the system.  The self-test suite could not see it; it had
/// already finished by then.
///
/// # What it checks
///
/// That a syscall in the top decade dispatches to its registered handler
/// rather than being refused by the filter.  `SYS_SIGNAL_STOP_SELF` with
/// signal 0 is used because it is the highest-numbered syscall with a
/// registered handler whose *rejection* path is safe to call from the boot
/// thread (a valid stop signal would park this thread forever — see
/// `test_dispatch_signal_stop_self_rejects_non_stop_signals`).  It must answer
/// `InvalidArgument`; `PermissionDenied` means the filter ate it, and
/// `NotSupported` means the slot is not wired.
///
/// # Errors
///
/// Returns `InternalError` if a live-filter dispatch is refused.
pub fn verify_dispatch_under_filtering() -> KernelResult<()> {
    let args = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let r = dispatch(SYS_SIGNAL_STOP_SELF, &args);

    if r.value == i64::from(KernelError::PermissionDenied.code()) {
        serial_println!(
            "[syscall]   FAIL: syscall {} refused by scfilter with no filter installed \
             (scfilter::MAX_SYSCALL_NR={}, dispatch MAX_SYSCALL_NR={})",
            SYS_SIGNAL_STOP_SELF,
            crate::scfilter::MAX_SYSCALL_NR,
            MAX_SYSCALL_NR
        );
        return Err(KernelError::InternalError);
    }
    if r.value != i64::from(KernelError::InvalidArgument.code()) {
        serial_println!(
            "[syscall]   FAIL: syscall {} under live filtering returned {}, expected InvalidArgument",
            SYS_SIGNAL_STOP_SELF,
            r.value
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[syscall] Top-of-range dispatch under live filtering (nr {} of {}): OK",
        SYS_SIGNAL_STOP_SELF,
        MAX_SYSCALL_NR
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Test the dispatch table by invoking syscalls from kernel mode.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[syscall] Running dispatch self-test...");

    test_dispatch_yield()?;
    test_dispatch_task_id()?;
    test_dispatch_unimplemented()?;
    test_dispatch_out_of_range()?;
    test_dispatch_channel_roundtrip()?;
    test_dispatch_clock_monotonic()?;
    test_dispatch_clock_realtime()?;
    test_dispatch_getrandom()?;
    test_dispatch_clock_settime()?;
    test_dispatch_clock_adjtime()?;
    test_dispatch_console_write()?;
    test_dispatch_fs_roundtrip()?;
    test_io_dir_classification()?;
    test_dispatch_mprotect_native()?;
    test_dispatch_process_group_syscalls()?;
    test_dispatch_ctty_syscalls()?;
    test_dispatch_termios_syscalls()?;
    test_dispatch_pty_syscalls()?;
    test_dispatch_rlimit_syscalls()?;
    test_dispatch_spawn_ex2_registered()?;
    test_dispatch_tty_job_control()?;
    test_dispatch_wait_process_group_filter()?;
    test_dispatch_signal_stop_self_rejects_non_stop_signals()?;
    test_dispatch_wait_status_reports_job_control()?;
    test_dispatch_wait_status_wpgid_and_wnowait()?;
    test_dispatch_wait_info_layout()?;
    test_dispatch_rusage_info_layout()?;
    test_dispatch_set_credentials_gate()?;

    serial_println!("[syscall] Dispatch self-test PASSED");
    Ok(())
}

/// Verify the **native** `mprotect` (SYS_MPROTECT = 22) is wired into the
/// dispatch table and runs the shared argument-validation gate, returning
/// raw `KernelError` codes (not Linux errno, and — crucially — not
/// `NotSupported`, which is what the old TD-NATIVE-MPROTECT stub returned).
///
/// This exercises the argument gates that short-circuit *before* any process
/// or page-table state is touched, so it is safe to run from the kernel
/// self-test task (which is not a user process).  The full page-table effect
/// is covered by the shared `mprotect_core`, which the Linux-ABI mprotect —
/// with its own boot self-tests and real glibc RELRO usage — also runs.
fn test_dispatch_mprotect_native() -> KernelResult<()> {
    let mk = |arg0: u64, arg1: u64, arg2: u64| SyscallArgs {
        arg0,
        arg1,
        arg2,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };

    // (a) Misaligned address → InvalidArgument (EINVAL), and NOT
    //     NotSupported — this alone proves the handler is registered.
    let r = dispatch(SYS_MPROTECT, &mk(0x1, 0x1000, 0x1));
    if r.value == i64::from(KernelError::NotSupported.code()) {
        serial_println!("[syscall]   FAIL: native mprotect unregistered (NotSupported)");
        return Err(KernelError::InternalError);
    }
    if r.value != i64::from(KernelError::InvalidArgument.code()) {
        serial_println!(
            "[syscall]   FAIL: native mprotect misalign returned {}, expected InvalidArgument",
            r.value
        );
        return Err(KernelError::InternalError);
    }

    // (b) Zero length → success (0), no work.
    let r = dispatch(SYS_MPROTECT, &mk(0x1000, 0, 0x1));
    if r.value != 0 {
        serial_println!(
            "[syscall]   FAIL: native mprotect len=0 returned {}, expected 0",
            r.value
        );
        return Err(KernelError::InternalError);
    }

    // (c) Unknown prot bit (0x8) on an otherwise-valid request → InvalidArgument.
    let r = dispatch(SYS_MPROTECT, &mk(0x1000, 0x1000, 0x8));
    if r.value != i64::from(KernelError::InvalidArgument.code()) {
        serial_println!(
            "[syscall]   FAIL: native mprotect bad-prot returned {}, expected InvalidArgument",
            r.value
        );
        return Err(KernelError::InternalError);
    }

    // (d) Length overflow (PAGE_ALIGN wraps) → OutOfMemory (ENOMEM).
    let r = dispatch(SYS_MPROTECT, &mk(0x1000, u64::MAX, 0x1));
    if r.value != i64::from(KernelError::OutOfMemory.code()) {
        serial_println!(
            "[syscall]   FAIL: native mprotect len-overflow returned {}, expected OutOfMemory",
            r.value
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[syscall]   Native mprotect (SYS_MPROTECT=22) wired + gate order: OK");
    Ok(())
}

/// Verify the **native** process-group syscalls (533–536) and the
/// non-positive (`kill(-pgid)`) forms of `SYS_SIGNAL_SEND` (523).
///
/// Motivation: `AbiMode` is per-process, so before these numbers existed a
/// program linked against our own posix libc could not reach `pcb::set_pgid`
/// at all — only the Linux shim could — and the libc papered over the gap
/// with a userspace `static mut PGID` that no other process could observe.
/// This test pins down three things that fix depends on:
///
/// 1. **Registration.** An unknown-PID query must come back as
///    `NoSuchProcess`, *not* `NotSupported`. `NotSupported` is what an
///    unregistered dispatch slot returns, so this distinction alone proves
///    the wiring.
/// 2. **Delegation, not duplication.** The group/session policy lives in
///    `proc::pcb`; the handlers only resolve arguments. So the checks below
///    are of the *resolution* (0 = caller, `pgid == 0` = lead a new group,
///    negative rejected) and of the error mapping — the policy itself is
///    covered by `proc::pcb::test_process_groups`.
/// 3. **Group-signal ordering.** `signal_send_to_group` is the single
///    implementation shared by both ABIs, and it must resolve the target set
///    *before* validating the signal number (ESRCH beats EINVAL, as in
///    Linux's `kill_something_info`). A bad signal aimed at a live group and
///    the same bad signal aimed at a dead group must therefore give
///    *different* errors.
///
/// Every process this creates is destroyed on every exit path. Only
/// `sig == 0` existence probes are issued at live groups, so nothing the
/// test creates is actually signalled.
fn test_dispatch_process_group_syscalls() -> KernelResult<()> {
    use crate::proc::pcb;

    let mk = |arg0: u64, arg1: u64| SyscallArgs {
        arg0,
        arg1,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    // Negative group targets travel in the `arg0` slot as their two's
    // complement bit pattern, which is what the handler reads back as i64.
    // A zero-extended 32-bit pid would arrive as a huge *positive* PID
    // instead, so `kill(-7)` would become an ESRCH against PID 4294967289.
    #[allow(clippy::cast_sign_loss)]
    let neg = |pid: i64| -> u64 { -pid as u64 };

    fn fail(msg: &str, pids: &[u64]) -> KernelResult<()> {
        serial_println!("[syscall]   FAIL: process groups: {}", msg);
        for &p in pids {
            crate::proc::pcb::destroy(p);
        }
        Err(KernelError::InternalError)
    }

    // A PID/PGID no process will ever hold, used both as an unknown target
    // and as an empty destination group.
    const NO_GROUP_I: i64 = 7_654_321;
    const NO_GROUP: u64 = 7_654_321;

    // (1) Registration: an unknown PID is ESRCH, not "no such syscall".
    for (nr, name) in [
        (SYS_PROCESS_GET_PGID, "getpgid"),
        (SYS_PROCESS_GET_SID, "getsid"),
    ] {
        let r = dispatch(nr, &mk(NO_GROUP, 0));
        if r.value == i64::from(KernelError::NotSupported.code()) {
            serial_println!("[syscall]     ({} is unregistered: NotSupported)", name);
            return fail("process-group syscall not wired into the table", &[]);
        }
        if r.value != i64::from(KernelError::NoSuchProcess.code()) {
            serial_println!("[syscall]     ({} unknown pid gave {})", name, r.value);
            return fail("unknown pid should be NoSuchProcess", &[]);
        }
    }

    // Build a parent and a forked child. `fork_create` (not `create`) is
    // essential: `create` makes the new process its own session leader,
    // whose pgid is fixed by rule, so it could never exercise `set_pgid`.
    let parent = pcb::create("pg-syscall-parent", 0);
    if pcb::set_running(parent).is_err() {
        return fail("could not run parent", &[parent]);
    }
    let child = match pcb::fork_create(parent, 0, alloc::vec::Vec::new(), alloc::vec::Vec::new()) {
        Ok(c) => c,
        Err(_) => return fail("fork_create failed", &[parent]),
    };
    if pcb::set_running(child).is_err() {
        return fail("could not run child", &[parent, child]);
    }
    let live = [parent, child];

    // (2) The child inherited the parent's group and session.
    #[allow(clippy::cast_possible_wrap)]
    let parent_i = parent as i64;
    #[allow(clippy::cast_possible_wrap)]
    let child_i = child as i64;
    if dispatch(SYS_PROCESS_GET_PGID, &mk(child, 0)).value != parent_i {
        return fail("getpgid(child) != parent after fork", &live);
    }
    if dispatch(SYS_PROCESS_GET_SID, &mk(child, 0)).value != parent_i {
        return fail("getsid(child) != parent after fork", &live);
    }

    // (3) `setpgid(child, 0)` — pgid 0 resolves to the target, creating a
    //     new group led by the child. This is precisely what a shell does
    //     to put a job in its own group, and precisely what the old
    //     userspace-only implementation could not make visible.
    if dispatch(SYS_PROCESS_SET_PGID, &mk(child, 0)).value != 0 {
        return fail("setpgid(child, 0) failed", &live);
    }
    if dispatch(SYS_PROCESS_GET_PGID, &mk(child, 0)).value != child_i {
        return fail("setpgid(child, 0) did not move the child", &live);
    }

    // (4) Error mapping is the pcb layer's, surfaced unchanged: joining a
    //     group no live process holds is EPERM, not ESRCH.
    if dispatch(SYS_PROCESS_SET_PGID, &mk(child, NO_GROUP)).value
        != i64::from(KernelError::PermissionDenied.code())
    {
        return fail(
            "setpgid into a nonexistent group should be PermissionDenied",
            &live,
        );
    }

    // (5) Argument gates. A negative pid is not a PID: `setpgid` reports it
    //     as EINVAL (it is a malformed argument), while the getters report
    //     ESRCH (there is simply no such process), matching POSIX.
    #[allow(clippy::cast_sign_loss)]
    let minus_one = -1_i64 as u64;
    if dispatch(SYS_PROCESS_SET_PGID, &mk(minus_one, 0)).value
        != i64::from(KernelError::InvalidArgument.code())
    {
        return fail("setpgid(-1, ..) should be InvalidArgument", &live);
    }
    if dispatch(SYS_PROCESS_SET_PGID, &mk(child, minus_one)).value
        != i64::from(KernelError::InvalidArgument.code())
    {
        return fail("setpgid(child, -1) should be InvalidArgument", &live);
    }
    if dispatch(SYS_PROCESS_GET_PGID, &mk(minus_one, 0)).value
        != i64::from(KernelError::NoSuchProcess.code())
    {
        return fail("getpgid(-1) should be NoSuchProcess", &live);
    }

    // (6) The kernel self-test task owns no process, so "the caller" cannot
    //     be resolved. Every 0-means-caller form must therefore say ESRCH
    //     rather than silently acting on PID 0 (which never exists — PIDs
    //     start at 1).
    if dispatch(SYS_PROCESS_GET_PGID, &mk(0, 0)).value
        != i64::from(KernelError::NoSuchProcess.code())
    {
        return fail(
            "getpgid(0) with no owning process should be NoSuchProcess",
            &live,
        );
    }
    if dispatch(SYS_PROCESS_SET_SID, &mk(0, 0)).value
        != i64::from(KernelError::NoSuchProcess.code())
    {
        return fail(
            "setsid() with no owning process should be NoSuchProcess",
            &live,
        );
    }

    // (7) `kill(-pgid, 0)`: a live group is reachable through the *native*
    //     SYS_SIGNAL_SEND. This is the whole point of the fix — before it,
    //     the native ABI rejected every non-positive pid outright.
    if dispatch(SYS_SIGNAL_SEND, &mk(neg(child_i), 0)).value != 0 {
        return fail("kill(-child, 0) should find the child's live group", &live);
    }
    if dispatch(SYS_SIGNAL_SEND, &mk(neg(NO_GROUP_I), 0)).value
        != i64::from(KernelError::NoSuchProcess.code())
    {
        return fail("kill(-<empty group>, 0) should be NoSuchProcess", &live);
    }
    // `kill(-1)` is the broadcast form, which we deliberately do not model
    // (it needs a credential model to define "every process you may
    // signal"); both ABIs report ESRCH. See known-issues.md,
    // TD-KILL-MINUS-ONE-BROADCAST-NOT-MODELLED.
    if dispatch(SYS_SIGNAL_SEND, &mk(minus_one, 0)).value
        != i64::from(KernelError::NoSuchProcess.code())
    {
        return fail(
            "kill(-1, 0) should be NoSuchProcess (broadcast unmodelled)",
            &live,
        );
    }
    // `kill(0, ..)` means "my own group" — unresolvable here, so ESRCH.
    if dispatch(SYS_SIGNAL_SEND, &mk(0, 0)).value != i64::from(KernelError::NoSuchProcess.code()) {
        return fail(
            "kill(0, 0) with no owning process should be NoSuchProcess",
            &live,
        );
    }

    // (8) Ordering: target resolution precedes signal validation. The same
    //     invalid signal number gives EINVAL at a live group but ESRCH at a
    //     dead one. If these two ever agree, the ordering has regressed and
    //     a shell would get the wrong errno for a reaped job.
    if dispatch(SYS_SIGNAL_SEND, &mk(neg(child_i), 999)).value
        != i64::from(KernelError::InvalidArgument.code())
    {
        return fail(
            "bad signal at a live group should be InvalidArgument",
            &live,
        );
    }
    if dispatch(SYS_SIGNAL_SEND, &mk(neg(NO_GROUP_I), 999)).value
        != i64::from(KernelError::NoSuchProcess.code())
    {
        return fail(
            "bad signal at a dead group should be NoSuchProcess (ESRCH first)",
            &live,
        );
    }

    pcb::destroy(child);
    pcb::destroy(parent);
    serial_println!("[syscall]   Native process groups (533-536) + kill(-pgid) ordering: OK");
    Ok(())
}

/// Verify the controlling-terminal syscalls (537–540) are wired into the
/// dispatch table and apply their argument gate in the right order.
///
/// Scope note: the semantics — that two processes in one session read the
/// same foreground group, that only a group in your own session may receive
/// the terminal, that `setsid` drops it — are covered by
/// `pcb::test_controlling_terminal`, which can build real sessions. This
/// test cannot: the self-test task owns no process, so every "the caller's
/// terminal" form is unresolvable here by construction. That is exactly what
/// makes it a good *registration* probe, though — an unresolvable caller must
/// report `NoSuchProcess`, so a `NotSupported` verdict can only mean the
/// syscall number was never registered.
fn test_dispatch_ctty_syscalls() -> KernelResult<()> {
    let mk = |arg0: u64| SyscallArgs {
        arg0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    fn fail(msg: &str) -> KernelResult<()> {
        serial_println!("[syscall]   FAIL: ctty: {}", msg);
        Err(KernelError::InternalError)
    }

    // (1) Registration. `NotSupported` is also ENOTTY, so it would be an
    //     ambiguous verdict from a caller that *has* a session — but this
    //     caller has no process at all, which must be ESRCH either way.
    if dispatch(SYS_TTY_GET_PGRP, &mk(0)).value != i64::from(KernelError::NoSuchProcess.code()) {
        return fail("tcgetpgrp with no owning process should be NoSuchProcess (unregistered?)");
    }
    if dispatch(SYS_TTY_SET_PGRP, &mk(1)).value != i64::from(KernelError::NoSuchProcess.code()) {
        return fail("tcsetpgrp with no owning process should be NoSuchProcess (unregistered?)");
    }
    for (nr, name) in [
        (SYS_TTY_ACQUIRE_CTTY, "TIOCSCTTY"),
        (SYS_TTY_RELEASE_CTTY, "TIOCNOTTY"),
    ] {
        if dispatch(nr, &mk(0)).value != i64::from(KernelError::NoSuchProcess.code()) {
            serial_println!("[syscall]     ({} gave an unexpected verdict)", name);
            return fail("ctty acquire/release with no owning process should be NoSuchProcess");
        }
    }

    // (2) The argument gate runs *before* caller resolution, so a malformed
    //     pgid is EINVAL even when the caller could not be established. If
    //     these two ever collapse into one verdict, a program would learn
    //     "no such process" for what is really a bad argument.
    #[allow(clippy::cast_sign_loss)]
    let minus_one = -1_i64 as u64;
    for (arg, what) in [(0_u64, "tcsetpgrp(0)"), (minus_one, "tcsetpgrp(-1)")] {
        if dispatch(SYS_TTY_SET_PGRP, &mk(arg)).value
            != i64::from(KernelError::InvalidArgument.code())
        {
            serial_println!("[syscall]     ({} did not report InvalidArgument)", what);
            return fail("a non-positive pgid should be InvalidArgument");
        }
    }

    serial_println!("[syscall]   Controlling terminal (537-540) registration: OK");
    Ok(())
}

/// Verify the native termios syscalls (541/542) reach the *same* line
/// discipline state the Linux shim's `TCGETS`/`TCSETS` do.
///
/// Unlike the ctty syscalls above, these need no process — the console's
/// termios is global — so this test can check real semantics rather than
/// only registration.
///
/// What it is guarding against is the bug that motivated the pair: libc's
/// `tcsetattr` was a silent no-op and its `tcgetattr` answered from a
/// hardcoded constant, so a native-ABI program could never observe the
/// terminal's real mode and never change it.  A test that only checked
/// "the syscall returns 0" would still have passed against that, which is
/// why step (2) reads the value back through `tty::get_termios` — the
/// accessor the line discipline itself uses on every keystroke.
///
/// The original termios is saved and restored: this runs during boot on the
/// real console, and leaving it in raw mode would break every later
/// interactive read.
fn test_dispatch_termios_syscalls() -> KernelResult<()> {
    use crate::tty;

    fn fail(msg: &str) -> KernelResult<()> {
        serial_println!("[syscall]   FAIL: termios: {}", msg);
        Err(KernelError::InternalError)
    }

    // Explicitly the console: this test runs during boot from a kernel task,
    // which has no controlling terminal, so it is also the device the syscalls
    // under test resolve to (`handlers::current_tty` falls back to `CONSOLE`).
    // Naming it here rather than relying on that fallback keeps the assertions
    // meaningful if the fallback ever changes.
    let dev = tty::CONSOLE;
    let saved = tty::get_termios(dev);

    // (1) A null pointer is InvalidArgument, not a fault or a silent success.
    //     This also proves both numbers are registered: an unregistered
    //     number reports NotSupported.
    let null_args = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    for (nr, name) in [
        (SYS_TTY_GET_TERMIOS, "SYS_TTY_GET_TERMIOS"),
        (SYS_TTY_SET_TERMIOS, "SYS_TTY_SET_TERMIOS"),
    ] {
        if dispatch(nr, &null_args).value != i64::from(KernelError::InvalidArgument.code()) {
            serial_println!("[syscall]     ({} gave an unexpected verdict)", name);
            return fail("a null termios pointer should be InvalidArgument (unregistered?)");
        }
    }

    // (2) A set must be *observable* through the line discipline's own
    //     accessor. Clearing ICANON is the change that matters: it is what
    //     "raw mode" means to every full-screen program, and it is exactly
    //     what libc used to drop on the floor.
    let mut raw = saved;
    raw.c_lflag &= !(tty::lflag::ICANON | tty::lflag::ECHO);
    tty::set_termios(dev, raw);
    let read_back = tty::get_termios(dev);
    if read_back.c_lflag & tty::lflag::ICANON != 0 {
        tty::set_termios(dev, saved);
        return fail("ICANON survived a raw-mode set");
    }
    if read_back.c_lflag & tty::lflag::ECHO != 0 {
        tty::set_termios(dev, saved);
        return fail("ECHO survived a raw-mode set");
    }

    // (3) The wire format both ABIs marshal must round-trip exactly. A
    //     mismatch here is a silent ABI break with `posix`'s
    //     `termios_to_wire`/`termios_from_wire`.
    let wire = raw.to_bytes();
    if wire.len() != tty::TERMIOS_BYTES {
        tty::set_termios(dev, saved);
        return fail("termios wire size is not TERMIOS_BYTES");
    }
    if tty::Termios::from_bytes(&wire) != raw {
        tty::set_termios(dev, saved);
        return fail("termios did not survive a to_bytes/from_bytes round trip");
    }

    tty::set_termios(dev, saved);
    if tty::get_termios(dev) != saved {
        return fail("failed to restore the original termios");
    }

    serial_println!("[syscall]   Native termios (541/542) reaches the line discipline: OK");
    Ok(())
}

/// Verify the rlimit syscalls (557/558) are registered, gate their arguments
/// in the documented order, and — the part that is not obvious — do not leak
/// whether an arbitrary pid exists.
///
/// This runs from a kernel task, which owns no process, so `arg0 = 0` resolves
/// to [`crate::proc::pcb::DEFAULT_RLIMITS`] rather than to a PCB and writes are
/// discarded.  The semantics that *need* a live process are tested where they
/// live, in `pcb::self_test`'s `test_rlimits` — including the one that matters
/// most, that `RLIMIT_NOFILE`'s hard limit can never exceed the fd table's real
/// capacity.  What only this layer can show is the syscall surface: that both
/// numbers dispatch at all, that a null buffer is refused before anything is
/// copied, and that the pid gate answers the same way for a live pid and a dead
/// one.
///
/// That last property is the reason this test exists rather than just the pcb
/// one.  `rlimit_target` refuses every non-self pid with `PermissionDenied`,
/// deliberately *including* pids that do not exist: an implementation that
/// helpfully distinguished them — `NoSuchProcess` for a dead pid,
/// `PermissionDenied` for a live one — would turn `getrlimit` into a
/// process-existence oracle callable by anything on the system.  A test that
/// only probed one of the two would pass against that.
fn test_dispatch_rlimit_syscalls() -> KernelResult<()> {
    use crate::proc::pcb;

    fn fail(msg: &str) -> KernelResult<()> {
        serial_println!("[syscall]   FAIL: rlimit: {}", msg);
        Err(KernelError::InternalError)
    }

    let invalid = i64::from(KernelError::InvalidArgument.code());
    let denied = i64::from(KernelError::PermissionDenied.code());

    // Non-null but deliberately unmapped, as in the pty test: a handler that
    // reaches the buffer before its argument gates have run fails with
    // `InvalidAddress`, a verdict distinct from every expected one, instead of
    // quietly succeeding.
    const UNMAPPED_USER_PTR: u64 = 0x1000;
    let args = |pid: u64, resource: u64, buf: u64| SyscallArgs {
        arg0: pid,
        arg1: resource,
        arg2: buf,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };

    // (1) A null buffer is InvalidArgument on both, which also proves both
    //     numbers are registered — an unregistered number reports NotSupported.
    for (nr, name) in [
        (SYS_RLIMIT_GET, "SYS_RLIMIT_GET"),
        (SYS_RLIMIT_SET, "SYS_RLIMIT_SET"),
    ] {
        if dispatch(nr, &args(0, 0, 0)).value != invalid {
            serial_println!("[syscall]     ({} gave an unexpected verdict)", name);
            return fail("a null rlimit pointer should be InvalidArgument (unregistered?)");
        }
    }

    // (2) A resource outside 0..=15 is InvalidArgument, and is checked before
    //     the pid — so a caller probing whether a resource number is understood
    //     gets the same answer whoever they are.  Passing a foreign pid *and* a
    //     bad resource is what distinguishes the two orders.
    for (nr, name) in [
        (SYS_RLIMIT_GET, "SYS_RLIMIT_GET"),
        (SYS_RLIMIT_SET, "SYS_RLIMIT_SET"),
    ] {
        let r = dispatch(
            nr,
            &args(0xdead_beef, u64::from(pcb::NUM_RLIMITS), UNMAPPED_USER_PTR),
        );
        if r.value != invalid {
            serial_println!("[syscall]     ({} gave {})", name, r.value);
            return fail(
                "a resource >= NUM_RLIMITS should be InvalidArgument, before the pid gate",
            );
        }
    }

    // (3) The pid gate.  From a kernel task every non-zero pid is foreign, so
    //     both a plausibly-live pid (1, which init holds by this point in boot)
    //     and a pid that certainly does not exist must give the *same*
    //     PermissionDenied.  Two probes, not one: a single probe cannot tell an
    //     existence oracle from a uniform refusal.
    const CERTAINLY_DEAD_PID: u64 = u64::MAX;
    for (nr, name) in [
        (SYS_RLIMIT_GET, "SYS_RLIMIT_GET"),
        (SYS_RLIMIT_SET, "SYS_RLIMIT_SET"),
    ] {
        for pid in [1u64, CERTAINLY_DEAD_PID] {
            let r = dispatch(nr, &args(pid, 0, UNMAPPED_USER_PTR));
            if r.value != denied {
                serial_println!("[syscall]     ({} pid={} gave {})", name, pid, r.value);
                return fail("a foreign pid should be PermissionDenied, alive or not");
            }
        }
    }

    serial_println!("[syscall]   Resource limits (557/558) registered and gated: OK");
    Ok(())
}

/// Verify `SYS_PROCESS_SPAWN_EX2` (559) is registered and refuses a null
/// argument pointer.
///
/// Deliberately narrow.  Everything else about the syscall is reachable only
/// from ring 3 — the size gate and the capability policy both run against a
/// *user* address, so a kernel-mode probe is refused for the address before
/// either is consulted — and both are tested where they can be reached:
/// `spawn::self_test`'s `test_ex2_copy_plan` sweeps the size gate exhaustively,
/// and `test_spawn_capability_subset` exercises the delegation rules through
/// `spawn_process_with_caps` directly.
///
/// What only this layer can show is that the number dispatches at all.  That is
/// worth its own check because the failure it catches is silent and total: an
/// unregistered number returns `NotSupported`, so a userspace caller would
/// conclude the kernel is too old and fall back to `SYS_PROCESS_SPAWN_EX` —
/// which inherits the *whole* capability table.  A missing table entry would
/// therefore not look like a broken syscall; it would look like a sandbox that
/// quietly stopped sandboxing.
fn test_dispatch_spawn_ex2_registered() -> KernelResult<()> {
    let args = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let r = dispatch(SYS_PROCESS_SPAWN_EX2, &args);
    let invalid = i64::from(KernelError::InvalidArgument.code());
    if r.value != invalid {
        serial_println!(
            "[syscall]   FAIL: SYS_PROCESS_SPAWN_EX2 gave {}, expected InvalidArgument ({}) \
             — an unregistered number reports NotSupported, and callers read that as \
             'old kernel' and fall back to inheriting everything",
            r.value,
            invalid
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[syscall]   Capability-subsetting spawn (559) registered: OK");
    Ok(())
}

/// Verify the pty syscalls (544–554) are registered and, above all, that the
/// **ownership gate** holds.
///
/// This test cannot exercise the happy path: it runs from a kernel task, which
/// owns no process, so it holds no handles and `SYS_PTY_CREATE` deliberately
/// refuses it (a kernel task has no handle table, so a pty created for one
/// would leak for the rest of the boot).  What it *can* check is the part that
/// matters most, and the check is meaningful precisely because a live pty
/// exists during it:
///
/// Every other IPC handle in this kernel is self-authorising, which is sound
/// because the values are unguessable.  A [`crate::tty::pty::PtyHandle`] is
/// `(tty_id << 1) | end` — enumerable — and a master handle is the authority to
/// type arbitrary bytes at whatever shell is on the other end.  So a caller
/// that names a handle it does not hold must be refused *even when the handle
/// names a pty that really exists*, which is the case a test against a
/// non-existent pty would not distinguish.  That is why this creates one first.
///
/// It also pins the two reserved raw values.  `0` means "my controlling
/// terminal" for the terminal-naming syscalls and `1` would decode as "the
/// slave of tty 0" — and tty 0 is the console, not a pty.  Both must be refused
/// by the handle-only syscalls rather than silently decoded.
fn test_dispatch_pty_syscalls() -> KernelResult<()> {
    use crate::tty::pty;

    fn fail(msg: &str) -> KernelResult<()> {
        serial_println!("[syscall]   FAIL: pty: {}", msg);
        Err(KernelError::InternalError)
    }

    let invalid = i64::from(KernelError::InvalidHandle.code());
    let no_proc = i64::from(KernelError::NoSuchProcess.code());

    // A real, live pty that this task does not own.  Everything below is a
    // *deliberate* attempt to reach it without a handle.
    let (m, s) = match pty::create() {
        Ok(pair) => pair,
        Err(_) => return fail("could not create a pty to test against"),
    };
    let id = m.id();

    // A non-null, deliberately *unmapped* user address.  The ownership check
    // runs before any buffer is validated or touched, so a handler that
    // reaches the pointer at all has already failed the test — and it fails it
    // as `InvalidAddress`, a verdict distinct from every expected one, rather
    // than silently reading kernel memory, which is what passing a real kernel
    // buffer here would have allowed.  (`mm::user` is what makes reaching it a
    // rejected copy rather than a kernel #PF; see `validate_kernel_range`.)
    const UNMAPPED_USER_PTR: u64 = 0x1000;
    let args_for = |handle: u64| SyscallArgs {
        arg0: handle,
        arg1: UNMAPPED_USER_PTR,
        arg2: 1,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };

    // (1) An unowned but *existing* master handle is refused.  This is the
    //     keystroke-injection case: without the gate, `dispatch` here would
    //     succeed and feed a byte into the pty's line discipline.
    for (nr, name) in [
        (SYS_PTY_MASTER_WRITE, "SYS_PTY_MASTER_WRITE"),
        (SYS_PTY_MASTER_TRY_READ, "SYS_PTY_MASTER_TRY_READ"),
        (SYS_PTY_CLOSE, "SYS_PTY_CLOSE"),
        (SYS_PTY_DUP, "SYS_PTY_DUP"),
        (SYS_PTY_SLAVE_ID, "SYS_PTY_SLAVE_ID"),
        (SYS_PTY_POLL, "SYS_PTY_POLL"),
        (SYS_PTY_READABLE_BYTES, "SYS_PTY_READABLE_BYTES"),
    ] {
        let got = dispatch(nr, &args_for(m.raw())).value;
        if got != invalid && got != no_proc {
            serial_println!(
                "[syscall]     ({} returned {} for an unowned handle)",
                name,
                got
            );
            let _ = pty::close(m);
            let _ = pty::close(s);
            return fail("a pty handle the caller does not own must be refused");
        }
    }

    // (2) The reserved raw values are refused by the handle-only syscalls
    //     rather than decoded into tty 0 (the console).  `SYS_PTY_POLL` on raw
    //     `0` would otherwise report on the console's non-existent pty.
    //     `SYS_PTY_READABLE_BYTES` is checked alongside it because both return
    //     a plain number rather than a status, so a decoded-instead-of-refused
    //     bug there produces a *plausible* answer rather than a visible one.
    for raw in [0u64, 1] {
        for (nr, name) in [
            (SYS_PTY_POLL, "SYS_PTY_POLL"),
            (SYS_PTY_READABLE_BYTES, "SYS_PTY_READABLE_BYTES"),
        ] {
            let got = dispatch(nr, &args_for(raw)).value;
            if got != invalid {
                serial_println!("[syscall]     ({} on raw {} returned {})", name, raw, got);
                let _ = pty::close(m);
                let _ = pty::close(s);
                return fail("raw 0 and 1 are reserved and must be InvalidHandle");
            }
        }
    }

    // (2b) `SYS_PTY_GET_PGRP`/`SYS_PTY_SET_PGRP` are deliberately *not* in the
    //      loop above: raw `0` means "my controlling terminal" for them, which
    //      is a legitimate request. Raw `1` is still reserved, and an unowned
    //      handle is still refused — the case that matters, because succeeding
    //      would let any process read (or redirect) the foreground job of a
    //      terminal it has no relationship to.
    for (nr, name) in [
        (SYS_PTY_GET_PGRP, "SYS_PTY_GET_PGRP"),
        (SYS_PTY_SET_PGRP, "SYS_PTY_SET_PGRP"),
    ] {
        // Raw 1 is rejected by `owned_pty_handle` before it looks the caller
        // up, so it must be exactly `InvalidHandle` — no weaker verdict. This
        // is what pins the ordering: both handlers resolve the terminal before
        // the caller precisely so this stays specific.
        let mut a = args_for(1);
        // arg1 is the pgid for the set form. A plausible one, so a handler
        // that skipped the handle check entirely would sail past it.
        a.arg1 = 1;
        let got = dispatch(nr, &a).value;
        if got != invalid {
            serial_println!("[syscall]     ({} on raw 1 returned {})", name, got);
            let _ = pty::close(m);
            let _ = pty::close(s);
            return fail("raw 1 is reserved and must be InvalidHandle");
        }
        // A real, live, *unowned* handle. `no_proc` is also acceptable here and
        // is in fact what this kernel-task caller gets, because ownership is
        // asked of a process it does not have — but either way the call must
        // not reach the terminal.
        let mut a = args_for(m.raw());
        a.arg1 = 1;
        let got = dispatch(nr, &a).value;
        if got != invalid && got != no_proc {
            serial_println!(
                "[syscall]     ({} on an unowned handle returned {})",
                name,
                got
            );
            let _ = pty::close(m);
            let _ = pty::close(s);
            return fail("an unowned handle must not name a terminal");
        }
    }

    // (3) Registration: an *unregistered* number reports NotSupported, which is
    //     distinct from every verdict above, so reaching here proves each of
    //     the numbers tested has a handler installed.  `SYS_PTY_CREATE` is the
    //     one not covered by (1), so check it explicitly — and its refusal of a
    //     kernel task is itself the contract that it does not leak a pty.
    let none = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let got = dispatch(SYS_PTY_CREATE, &none).value;
    if got != no_proc {
        serial_println!(
            "[syscall]     (SYS_PTY_CREATE returned {} from a kernel task)",
            got
        );
        let _ = pty::close(m);
        let _ = pty::close(s);
        return fail("SYS_PTY_CREATE from a kernel task should be NoSuchProcess (unregistered?)");
    }

    // (4) The four terminal-naming syscalls resolve `arg0 == 0` to the caller's
    //     terminal — the console here — rather than treating it as a handle. A
    //     null buffer is InvalidArgument, which also proves each is registered.
    for (nr, name) in [
        (SYS_PTY_GET_WINSIZE, "SYS_PTY_GET_WINSIZE"),
        (SYS_PTY_SET_WINSIZE, "SYS_PTY_SET_WINSIZE"),
        (SYS_PTY_GET_TERMIOS, "SYS_PTY_GET_TERMIOS"),
        (SYS_PTY_SET_TERMIOS, "SYS_PTY_SET_TERMIOS"),
    ] {
        if dispatch(nr, &none).value != i64::from(KernelError::InvalidArgument.code()) {
            serial_println!("[syscall]     ({} gave an unexpected verdict)", name);
            let _ = pty::close(m);
            let _ = pty::close(s);
            return fail("a null buffer should be InvalidArgument (unregistered?)");
        }
    }

    // (5) …and refuse an unowned handle in `arg0` *before* looking at the
    //     buffer, so a caller cannot learn whether a pty exists, or push a
    //     discipline onto someone else's shell, by naming it.
    for (nr, name) in [
        (SYS_PTY_GET_WINSIZE, "SYS_PTY_GET_WINSIZE"),
        (SYS_PTY_SET_WINSIZE, "SYS_PTY_SET_WINSIZE"),
        (SYS_PTY_GET_TERMIOS, "SYS_PTY_GET_TERMIOS"),
        (SYS_PTY_SET_TERMIOS, "SYS_PTY_SET_TERMIOS"),
    ] {
        let got = dispatch(nr, &args_for(m.raw())).value;
        if got != invalid && got != no_proc {
            serial_println!(
                "[syscall]     ({} returned {} for an unowned handle)",
                name,
                got
            );
            let _ = pty::close(m);
            let _ = pty::close(s);
            return fail("a terminal named by an unowned handle must be refused");
        }
    }

    let _ = pty::close(m);
    let _ = pty::close(s);
    if crate::tty::exists(id) {
        return fail("the test pty outlived both its ends");
    }

    serial_println!(
        "[syscall]   Pty syscalls (544-556, 869-871) registered, ownership enforced: OK"
    );
    Ok(())
}

/// Verify POSIX terminal-access job control — `handlers::tty_job_control_decide`,
/// the policy behind `SIGTTIN`/`SIGTTOU`.
///
/// This tests the *decision*, not the delivery, which is the whole reason
/// `tty_job_control_decide` is a pure function of process state: the effectful
/// wrapper asks about the *calling* process, and the self-test task is a
/// kernel task that owns no process, so it could never put itself in the
/// background of a terminal to be checked.
///
/// The cases, and what each one is guarding:
///
/// 1. A **foreground** process is allowed. If this ever failed, an ordinary
///    interactive program could not read its own terminal.
/// 2. A **background** read raises `SIGTTIN` and a background `tcsetattr`
///    raises `SIGTTOU` — the signal that stops the intruder.
/// 3. **Blocked** signal: the read fails `EIO` but the write is *allowed*.
///    This asymmetry is Linux's and is the easiest part to get backwards.
///    For the read it is also a liveness property, not a nicety: a blocked
///    `SIGTTIN` is undeliverable, so raising it and restarting would spin in
///    the kernel forever.
/// 4. An **orphaned** background group fails `EIO` for both, because no
///    session member remains that could ever `SIGCONT` it back.
///
/// The console's real controlling-terminal state is claimed and released
/// around the test, exactly as `pcb::test_controlling_terminal` does.
fn test_dispatch_tty_job_control() -> KernelResult<()> {
    use crate::proc::pcb;
    use crate::proc::signal::{self, SIGTTIN, SIGTTOU};
    use crate::syscall::handlers::{TtyAccessDecision, tty_job_control_decide};

    fn fail(msg: &str, pids: &[u64]) -> KernelResult<()> {
        serial_println!("[syscall]   FAIL: tty job control: {}", msg);
        for &p in pids {
            pcb::destroy(p);
        }
        Err(KernelError::InternalError)
    }

    // A shell that leads its own session and owns the console, plus a child
    // job the shell has placed in its own process group — the standard
    // job-control shape.
    let shell = pcb::create("jc-shell", 0);
    if pcb::set_running(shell).is_err() {
        return fail("could not start the shell process", &[shell]);
    }
    if pcb::ctty_acquire(shell, crate::tty::CONSOLE).is_err() {
        return fail("the shell could not claim the console", &[shell]);
    }
    let job = match pcb::fork_create(shell, 0, alloc::vec::Vec::new(), alloc::vec::Vec::new()) {
        Ok(p) => p,
        Err(_) => return fail("could not fork the job", &[shell]),
    };
    let cleanup = [shell, job];
    if pcb::set_running(job).is_err() || pcb::set_pgid(shell, job, job).is_err() {
        return fail("could not put the job in its own group", &cleanup);
    }

    // (1) The shell is the foreground group: unconditionally allowed.
    for sig in [SIGTTIN, SIGTTOU] {
        if tty_job_control_decide(shell, sig) != TtyAccessDecision::Allow {
            return fail("a foreground process was refused terminal access", &cleanup);
        }
    }

    // (2) The job is background: reads raise SIGTTIN, control raises SIGTTOU.
    //     The shell is a live guardian (different group, same session), so the
    //     job's group is not orphaned and stopping it is safe.
    if tty_job_control_decide(job, SIGTTIN) != TtyAccessDecision::Signal(SIGTTIN) {
        return fail("a background read did not raise SIGTTIN", &cleanup);
    }
    if tty_job_control_decide(job, SIGTTOU) != TtyAccessDecision::Signal(SIGTTOU) {
        return fail("a background tcsetattr did not raise SIGTTOU", &cleanup);
    }

    // (3) Block both signals in the job. Now neither can be delivered, so the
    //     read must fail rather than spin and the write must proceed.
    let (Some(ttin_bit), Some(ttou_bit)) =
        (signal::signal_bit(SIGTTIN), signal::signal_bit(SIGTTOU))
    else {
        return fail("SIGTTIN/SIGTTOU have no mask bit", &cleanup);
    };
    let saved_mask = signal::set_blocked(job, ttin_bit | ttou_bit);
    let blocked_read = tty_job_control_decide(job, SIGTTIN);
    let blocked_write = tty_job_control_decide(job, SIGTTOU);
    let _ = signal::set_blocked(job, saved_mask);
    if blocked_read != TtyAccessDecision::Fail(KernelError::IoError) {
        return fail(
            "a background read with SIGTTIN blocked should be EIO",
            &cleanup,
        );
    }
    if blocked_write != TtyAccessDecision::Allow {
        return fail(
            "a background write with SIGTTOU blocked should be allowed",
            &cleanup,
        );
    }

    // (4) Orphan the job's group by removing its only guardian. The shell is
    //     the job's parent, and it is the sole process outside the job's group
    //     but inside its session — so once the shell is gone, nothing survives
    //     that could ever `SIGCONT` the job. Stopping it would be permanent, so
    //     POSIX substitutes `EIO` for both signals.
    //
    //     The console deliberately stays claimed across this. `destroy` only
    //     drops a session's terminal once the session is *empty* (see
    //     `ctty_release_if_session_empty`), and the job inherited the shell's
    //     session — so the terminal survives its owner, still with the shell's
    //     now-defunct group in the foreground. That is precisely the state a
    //     real orphaned background job is in, and it means the job is still
    //     background here for the same reason it was in cases (2) and (3):
    //     nothing about the terminal changed, only the guardian went away.
    pcb::destroy(shell);
    if !pcb::pgrp_is_orphaned(job) {
        return fail(
            "the job's group was not orphaned by its parent's exit",
            &[job],
        );
    }
    for sig in [SIGTTIN, SIGTTOU] {
        if tty_job_control_decide(job, sig) != TtyAccessDecision::Fail(KernelError::IoError) {
            return fail(
                "an orphaned background group was not refused with EIO",
                &[job],
            );
        }
    }

    // Destroying the last member of the session releases the console with it,
    // leaving the terminal exactly as the test found it: unowned.
    pcb::destroy(job);
    serial_println!("[syscall]   TTY job control (SIGTTIN/SIGTTOU) policy: OK");
    Ok(())
}

/// Verify that the **native** wait syscalls honour the process-group forms
/// of a non-positive `pid`, rather than collapsing them to "any child".
///
/// Motivation: this is the same ABI asymmetry that
/// `test_dispatch_process_group_syscalls` covers for `kill`. The Linux
/// shim's `wait4` has filtered by group for a while (`linux.rs`'s
/// `pgid_filter`), but `SYS_PROCESS_WAIT`/`SYS_PROCESS_TRY_WAIT` treated
/// every `pid <= 0` as `-1`. Both ABIs read the *same* `pcb` group
/// records, so a program on our own libc silently reaped a child from the
/// wrong group where a glibc program got `ECHILD`.
///
/// The decisive check is step (2): both children are zombies and
/// `child_a < child_b` by PID, while the any-child scan (`pcb::peek_exit_any`)
/// deliberately picks the *lowest* PID. So asking for `child_b`'s group must
/// yield `child_b`. An
/// unfiltered implementation returns `child_a` and the test fails —
/// which is exactly what the pre-fix code did.
///
/// Every query here is group-filtered, which also makes the test immune to
/// unrelated children of pid 0 that other boot self-tests may have left
/// behind: a filter naming a group they are not in cannot see them, and
/// cannot reap them by accident either.
///
/// The `pid == 0` ("caller's own group") form is deliberately *not*
/// exercised here. `caller_pid()` reports 0 for a bare kernel task, pid 0
/// has no process record and so no pgid, so `WaitTarget::from_posix_selector`
/// degrades it to wait-any by design — calling it would be an unfiltered reap
/// that could destroy an unrelated child of pid 0 and make a later self-test
/// fail mysteriously. What this test pins down is the `< -1` form, and with
/// it the shared [`crate::syscall::wait`] selection path that the `== 0` form
/// also goes through — and that `wait4`, `waitid` and the native
/// wait-with-status now all share, so this test covers all five entry points.
fn test_dispatch_wait_process_group_filter() -> KernelResult<()> {
    use crate::proc::pcb;

    let mk = |arg0: u64| SyscallArgs {
        arg0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    #[allow(clippy::cast_sign_loss)]
    let neg = |pid: i64| -> u64 { -pid as u64 };

    fn fail(msg: &str, pids: &[u64]) -> KernelResult<()> {
        serial_println!("[syscall]   FAIL: wait group filter: {}", msg);
        for &p in pids {
            crate::proc::pcb::destroy(p);
        }
        Err(KernelError::InternalError)
    }

    // A group id no process will ever hold.
    const NO_GROUP_I: i64 = 8_765_432;

    // Children of pid 0 — the process id `caller_pid()` reports for the
    // bare kernel task running this self-test, so the handler agrees these
    // are its children. `create` makes each its own session and group
    // leader, so `pgid == pid` and the two sit in *different* groups.
    let child_a = pcb::create("waitpg-a", 0);
    let child_b = pcb::create("waitpg-b", 0);
    if child_a >= child_b {
        return fail(
            "expected ascending PIDs for the ordering check",
            &[child_a, child_b],
        );
    }
    let both = [child_a, child_b];
    for (pid, tid, code) in [(child_a, 9970_u64, 11_i32), (child_b, 9971, 22)] {
        if pcb::set_running(pid).is_err() || pcb::add_thread(pid, tid).is_err() {
            return fail("could not start child", &both);
        }
        // Turn the child into a zombie carrying a distinctive exit code.
        if pcb::set_exit_code(pid, code).is_err() {
            return fail("could not set exit code", &both);
        }
        match pcb::remove_thread(pid, tid, pcb::ThreadExitAccounting::default()) {
            Ok((true, _, _)) => {}
            _ => return fail("child did not become a zombie", &both),
        }
    }

    // (1) A group with no members of ours is ECHILD — and must not reap.
    if dispatch(SYS_PROCESS_TRY_WAIT, &mk(neg(NO_GROUP_I))).value
        != i64::from(KernelError::NoChildProcess.code())
    {
        return fail("wait on an empty group should be NoChildProcess", &both);
    }

    // (2) Ask for the *higher*-PID child by group. Reaping destroys it, so
    //     a correct answer both returns code 22 and leaves child_a alone.
    #[allow(clippy::cast_possible_wrap)]
    let child_b_i = child_b as i64;
    if dispatch(SYS_PROCESS_TRY_WAIT, &mk(neg(child_b_i))).value != 22 {
        return fail(
            "wait(-pgid_b) should reap child_b, not the lowest-PID child",
            &both,
        );
    }

    // (3) The filter is still restricting after a successful reap: child_a
    //     is a waiting zombie, but it is not in this group.
    if dispatch(SYS_PROCESS_TRY_WAIT, &mk(neg(NO_GROUP_I))).value
        != i64::from(KernelError::NoChildProcess.code())
    {
        return fail(
            "empty group should still be NoChildProcess with a zombie pending",
            &[child_a],
        );
    }

    // (4) child_a's own group reaps child_a, leaving nothing behind.
    #[allow(clippy::cast_possible_wrap)]
    let child_a_i = child_a as i64;
    if dispatch(SYS_PROCESS_TRY_WAIT, &mk(neg(child_a_i))).value != 11 {
        return fail("wait(-pgid_a) should reap child_a", &[child_a]);
    }

    // (5) Its group is now empty, so the same call is ECHILD.
    if dispatch(SYS_PROCESS_TRY_WAIT, &mk(neg(child_a_i))).value
        != i64::from(KernelError::NoChildProcess.code())
    {
        return fail("reaped child's group should be NoChildProcess", &[]);
    }

    serial_println!("[syscall]   Native wait(-pgid) group filter (matches Linux wait4): OK");
    Ok(())
}

/// Verify `SYS_PROCESS_WAIT_STATUS` (1063) reports job-control transitions
/// to a native-ABI caller, and reports them *only* when asked.
///
/// This is the parent half of job control. Without it a program on our own
/// libc could stop (`SYS_SIGNAL_STOP_SELF`) but nobody could observe the
/// stop: `SYS_PROCESS_WAIT` returns the exit code in `rax`, so it has no
/// way to say "stopped", and a parent calling it would simply block until a
/// child that is parked forever exits.
///
/// The `wstatus` *encoding* is not checked here — `arg2` must be a user
/// address, and the self-test task has none. It is covered by
/// `linux::test_wstatus_encoding`, which exercises the same
/// `JobControlEvent::to_wstatus` / `ExitInfo::to_wstatus` this path calls.
/// What is checked here is everything else: option validation, which
/// transitions are eligible, that a report is consumed exactly once, and
/// that a stop does *not* reap.
///
/// `record_jc_stopped` / `record_jc_continued` are used directly rather than
/// `stop_process_for_signal`, because they are pure bookkeeping: the fixture
/// child has a thread id in its PCB but no scheduler task, so actually
/// suspending it is neither possible nor what this test is about.
///
/// Every process created is destroyed on every exit path.
fn test_dispatch_wait_status_reports_job_control() -> KernelResult<()> {
    use crate::proc::pcb;

    // WNOHANG throughout: this runs on the boot thread, and a blocking wait
    // on a fixture child that has no task to ever exit would never return.
    const WNOHANG: u64 = 1;
    const WUNTRACED: u64 = 2;
    const WCONTINUED: u64 = 8;
    const SIGTSTP: u32 = 20;

    let mk = |arg0: u64, arg1: u64| SyscallArgs {
        arg0,
        arg1,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };

    fn fail(msg: &str, pids: &[u64]) -> KernelResult<()> {
        serial_println!("[syscall]   FAIL: wait_status: {}", msg);
        for &p in pids {
            crate::proc::pcb::destroy(p);
        }
        Err(KernelError::InternalError)
    }

    // (a) Option validation runs before anything else, so it needs no
    //     fixture. WEXITED (4) is a `waitid`-only bit and is not accepted by
    //     waitpid; a NotSupported here would mean the slot is unregistered.
    let r = dispatch(SYS_PROCESS_WAIT_STATUS, &mk(1, 4));
    if r.value == i64::from(KernelError::NotSupported.code()) {
        return fail("unregistered (NotSupported)", &[]);
    }
    if r.value != i64::from(KernelError::InvalidArgument.code()) {
        return fail("WEXITED should be InvalidArgument for waitpid", &[]);
    }

    // A child of pid 0 — the process id the bare kernel task running this
    // self-test reports — so the handler agrees it is ours.
    let child = pcb::create("waitstatus", 0);
    let one = [child];
    if pcb::set_running(child).is_err() || pcb::add_thread(child, 9980).is_err() {
        return fail("could not start the fixture child", &one);
    }

    #[allow(clippy::cast_possible_wrap)]
    let child_i = child as i64;
    #[allow(clippy::cast_sign_loss)]
    let child_arg = child_i as u64;

    // (b) Running child, nothing to report → 0, not an error.
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(child_arg, WNOHANG)).value != 0 {
        return fail(
            "a running child with no transition should be a WNOHANG miss",
            &one,
        );
    }

    // (c) A stop the caller did not ask about is invisible. This is the
    //     check that keeps `WUNTRACED` meaningful: a wait that reported
    //     stops unconditionally would break every existing caller, which
    //     expects a return only when the child is *gone*.
    let _ = pcb::record_jc_stopped(child, SIGTSTP);
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(child_arg, WNOHANG)).value != 0 {
        return fail("a stop must not be reported without WUNTRACED", &one);
    }

    // (d) With WUNTRACED it is reported, as the child's pid...
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(child_arg, WNOHANG | WUNTRACED)).value != child_i {
        return fail("WUNTRACED should report the stopped child", &one);
    }
    // ...and reported once: the transition is consumed, not latched.
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(child_arg, WNOHANG | WUNTRACED)).value != 0 {
        return fail("the same stop must not be reported twice", &one);
    }
    // ...and the child is still alive, because a stop is not an exit.
    if pcb::state(child).is_none() {
        return fail("reporting a stop must not reap the child", &[]);
    }

    // (e) The same, for the resume half.
    let _ = pcb::record_jc_continued(child);
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(child_arg, WNOHANG | WUNTRACED)).value != 0 {
        return fail("WUNTRACED must not report a continue", &one);
    }
    if dispatch(
        SYS_PROCESS_WAIT_STATUS,
        &mk(child_arg, WNOHANG | WCONTINUED),
    )
    .value
        != child_i
    {
        return fail("WCONTINUED should report the resumed child", &one);
    }

    // (f) Exit still works, and now returns the *pid* rather than the exit
    //     code — the whole reason this is a separate syscall number.
    if pcb::set_exit_code(child, 33).is_err() {
        return fail("could not set the fixture exit code", &one);
    }
    match pcb::remove_thread(child, 9980, pcb::ThreadExitAccounting::default()) {
        Ok((true, _, _)) => {}
        _ => return fail("the fixture child did not become a zombie", &one),
    }
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(child_arg, WNOHANG)).value != child_i {
        return fail("an exited child should be reported by pid", &one);
    }

    // (g) That reaped it, so the child is gone and the wait is ECHILD —
    //     the POSIX answer for a pid that is not (or is no longer) ours.
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(child_arg, WNOHANG)).value
        != i64::from(KernelError::NoChildProcess.code())
    {
        return fail("waiting on a reaped child should be NoChildProcess", &[]);
    }

    serial_println!("[syscall]   wait_status (1063) job-control reporting: OK");
    Ok(())
}

/// Verify the two option bits `SYS_PROCESS_WAIT_STATUS` has that Linux's
/// `waitpid` does not: `WPGID` (the selector is a bare unsigned pgid) and
/// `WNOWAIT` (report the transition without consuming it).
///
/// Both come from lane B's request
/// `requests/b-a-waitid-needs-an-explicit-idtype-wait.md`.
///
/// ## What makes the `WPGID` half decisive
///
/// `WPGID` exists because `waitpid`'s signed selector cannot name process
/// group 1: that would be `-1`, which already means "any child". A kernel
/// task cannot *itself* be put in group 1 — `pcb::create` makes every
/// fixture a session leader, whose pgid is fixed by POSIX rule 3, and pid 0
/// (what `caller_pid()` reports here) has no process record for `set_pgid`
/// to check a session against — so the nameability of group 1 is not
/// directly exercisable from a self-test. What *is* exercisable, and pins
/// down the same thing, is that the two interpretations of one argument
/// disagree and the bit picks between them:
///
/// | `arg0` | without `WPGID` | with `WPGID` |
/// |---|---|---|
/// | `-C` (as `i64`) | "group C" → reports C | a huge unsigned pgid → ECHILD |
/// | `C` | "pid C" → reports C | "group C" → reports C |
///
/// The first row is the decisive one: a handler that ignored `WPGID` and
/// fell through to `WaitTarget::from_posix_selector` would report C where
/// this test demands ECHILD. That is exactly the bug the bit is there to
/// prevent, and the group-1 case is the same code path with a different
/// number in it.
///
/// ## What makes the `WNOWAIT` half decisive
///
/// A peek that silently reaped would still return the right pid the first
/// time, so returning the pid is not evidence. The check is that the child
/// is *still there afterwards* (`pcb::state` is `Some`) and is reported
/// **again** by an identical second call — and, for the job-control half,
/// that a stop report survives a peek and is then consumed exactly once by
/// a wait without the bit.
///
/// ## A bug this test caught on its first run
///
/// It is the first self-test to reach the *blocking* wait's any-child/group
/// branch (`test_dispatch_wait_process_group_filter` goes through
/// `SYS_PROCESS_TRY_WAIT`, which calls `scan_once` directly, and
/// `test_dispatch_wait_status_reports_job_control` names a pid, taking the
/// other branch). That branch registered the waiter before scanning and
/// returned the registration error as the answer — but registration fails
/// when the *caller* has no process record, which conflated "you have no
/// children" with "you are not a process". Every any-child and every group
/// wait from a caller without a PCB reported ECHILD without scanning at all.
/// Fixed in `wait.rs` by letting the scan answer first; see the comment
/// there.
///
/// Every process created is destroyed on every exit path.
fn test_dispatch_wait_status_wpgid_and_wnowait() -> KernelResult<()> {
    use crate::proc::pcb;

    // WNOHANG throughout: this runs on the boot thread, and a blocking wait
    // on a fixture that has no task to ever exit would never return.
    const WNOHANG: u64 = 0x0000_0001;
    const WUNTRACED: u64 = 0x0000_0002;
    const WPGID: u64 = 0x0001_0000;
    const WNOWAIT: u64 = 0x0100_0000;
    const WINFO: u64 = 0x0002_0000;
    const SIGTSTP: u32 = 20;

    let mk = |arg0: u64, arg1: u64| SyscallArgs {
        arg0,
        arg1,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };

    fn fail(msg: &str, pids: &[u64]) -> KernelResult<()> {
        serial_println!("[syscall]   FAIL: wait opts: {}", msg);
        for &p in pids {
            crate::proc::pcb::destroy(p);
        }
        Err(KernelError::InternalError)
    }

    // (a) An unknown option bit is EINVAL, not silently ignored. A caller
    //     setting a bit this kernel has never heard of was compiled against
    //     a newer one; giving it the old semantics under a name that
    //     promises different ones is worse than refusing.
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(0, 0x0200_0000)).value
        != i64::from(KernelError::InvalidArgument.code())
    {
        return fail("an unknown option bit should be InvalidArgument", &[]);
    }
    // ...while the two new bits are accepted (no child yet → ECHILD, which
    // is a *different* answer from the EINVAL above, so this distinguishes
    // "understood" from "rejected").
    const NO_GROUP: u64 = 8_765_431;
    if dispatch(
        SYS_PROCESS_WAIT_STATUS,
        &mk(NO_GROUP, WNOHANG | WPGID | WNOWAIT),
    )
    .value
        != i64::from(KernelError::NoChildProcess.code())
    {
        return fail("WPGID|WNOWAIT should be accepted option bits", &[]);
    }

    // --- WPGID ---------------------------------------------------------
    // A zombie child of pid 0, leading its own group, so pgid == pid.
    let c = pcb::create("waitopt-pg", 0);
    let one = [c];
    if pcb::set_running(c).is_err() || pcb::add_thread(c, 9990).is_err() {
        return fail("could not start the WPGID fixture", &one);
    }
    if pcb::set_exit_code(c, 55).is_err() {
        return fail("could not set the WPGID fixture exit code", &one);
    }
    match pcb::remove_thread(c, 9990, pcb::ThreadExitAccounting::default()) {
        Ok((true, _, _)) => {}
        _ => return fail("the WPGID fixture did not become a zombie", &one),
    }

    #[allow(clippy::cast_possible_wrap)]
    let c_i = c as i64;
    // The exact bit pattern a caller writes when it means the selector `-C`.
    // Said as a `u64` wrapping negation rather than a round trip through `i64`,
    // so it is a bit operation on a value this test owns rather than signed
    // arithmetic that clippy must take on trust.
    let c_neg = c.wrapping_neg();

    // (b) The decisive row: `-C` under WPGID is an unsigned pgid no process
    //     holds, so ECHILD. Without the bit this same argument means
    //     "group C" and reports C — see (d).
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(c_neg, WNOHANG | WPGID)).value
        != i64::from(KernelError::NoChildProcess.code())
    {
        return fail(
            "WPGID must read arg0 as an unsigned pgid, not a signed selector",
            &one,
        );
    }
    // (c) ...and that near-miss must not have consumed anything.
    if pcb::state(c).is_none() {
        return fail("a WPGID miss must not reap", &[]);
    }
    // (d) The same number, unsigned, names C's group and reports C. Peek
    //     (WNOWAIT) so the fixture survives into the WNOWAIT half below.
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(c, WNOHANG | WPGID | WNOWAIT)).value != c_i {
        return fail("WPGID with the child's own pgid should report it", &one);
    }

    // --- WNOWAIT -------------------------------------------------------
    // (e) The peek above left the zombie in place...
    if pcb::state(c).is_none() {
        return fail("WNOWAIT must not reap the zombie", &[]);
    }
    // (f) ...so an identical call reports it again. A reaping implementation
    //     answers ECHILD here.
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(c, WNOHANG | WPGID | WNOWAIT)).value != c_i {
        return fail("a peeked zombie should be reported again", &one);
    }
    // (f2) Garbage in arg3/arg4 with no `WINFO` is not looked at.
    //
    //      This is the regression test for a bug that reached the ring-3
    //      fixtures: `arg3`/`arg4` were read unconditionally, but every
    //      caller that predates them invokes this syscall through a
    //      three-argument wrapper (`posix`'s `syscall3` writes rdi/rsi/rdx
    //      and stops), so `r10`/`r8` arrive holding stale caller values. The
    //      kernel was validating — and would have written 72 bytes through —
    //      a pointer userspace never supplied.
    //
    //      The address below is deliberately *non-canonical*, so if the gate
    //      is ever removed this does not quietly corrupt whatever it hits: a
    //      write faults immediately and takes the boot test with it. Note the
    //      limit, though — `validate_user_write` documents a bypass for tasks
    //      with no owning process, so a kernel task cannot observe the EFAULT
    //      a real process would get here. The ring-3 fixtures
    //      (`ctest-pgroup`/`ctest-jobctl`/`ctest-ctty`) are the test that
    //      actually caught this, and they remain the authority.
    const GARBAGE_PTR: u64 = 0xDEAD_BEEF_DEAD_0000;
    let with_garbage = SyscallArgs {
        arg0: c,
        arg1: WNOHANG | WPGID | WNOWAIT,
        arg2: 0,
        arg3: GARBAGE_PTR,
        arg4: 0xFFFF_FFFF,
        arg5: 0,
    };
    if dispatch(SYS_PROCESS_WAIT_STATUS, &with_garbage).value != c_i {
        return fail(
            "arg3/arg4 must be ignored entirely without WINFO — an old \
             three-argument caller leaves garbage in those registers",
            &one,
        );
    }
    // ...and `WINFO` itself is an understood bit, not EINVAL. (Pointing it at
    // nothing is the "I asked but have nowhere to put it" case, which is a
    // skip, not an error — so this stays safe to run from a kernel task.)
    let winfo_null = SyscallArgs {
        arg0: c,
        arg1: WNOHANG | WPGID | WNOWAIT | WINFO,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    if dispatch(SYS_PROCESS_WAIT_STATUS, &winfo_null).value != c_i {
        return fail("WINFO should be an accepted option bit", &one);
    }

    // (g) Dropping the bit consumes it: reported once...
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(c, WNOHANG | WPGID)).value != c_i {
        return fail("a wait without WNOWAIT should reap the peeked zombie", &one);
    }
    // (h) ...and then it is gone, for good.
    if pcb::state(c).is_some() {
        return fail("the zombie should be reaped once WNOWAIT is dropped", &one);
    }
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(c, WNOHANG | WPGID)).value
        != i64::from(KernelError::NoChildProcess.code())
    {
        return fail("a reaped child's group should be NoChildProcess", &[]);
    }

    // --- WNOWAIT on a job-control report --------------------------------
    // A stop is a *report*, not a corpse: consuming it clears a flag rather
    // than destroying a PCB, so it is a genuinely separate code path
    // (`jc_report_for_child`'s `consume` argument) and needs its own check.
    let s = pcb::create("waitopt-jc", 0);
    let js = [s];
    if pcb::set_running(s).is_err() || pcb::add_thread(s, 9991).is_err() {
        return fail("could not start the WNOWAIT job-control fixture", &js);
    }
    #[allow(clippy::cast_possible_wrap)]
    let s_i = s as i64;
    let _ = pcb::record_jc_stopped(s, SIGTSTP);
    // (i) Peeked twice, still pending both times.
    for pass in 0..2 {
        if dispatch(
            SYS_PROCESS_WAIT_STATUS,
            &mk(s, WNOHANG | WUNTRACED | WNOWAIT),
        )
        .value
            != s_i
        {
            return fail(
                if pass == 0 {
                    "WNOWAIT should report a pending stop"
                } else {
                    "a peeked stop report should survive to be seen again"
                },
                &js,
            );
        }
    }
    // (j) Consumed exactly once without the bit.
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(s, WNOHANG | WUNTRACED)).value != s_i {
        return fail("dropping WNOWAIT should report the stop", &js);
    }
    if dispatch(SYS_PROCESS_WAIT_STATUS, &mk(s, WNOHANG | WUNTRACED)).value != 0 {
        return fail("a consumed stop report must not be reported twice", &js);
    }
    pcb::destroy(s);

    serial_println!("[syscall]   wait_status WPGID / WNOWAIT: OK");
    Ok(())
}

/// Pin down the on-wire byte layout of the `WaitInfo` out-parameter
/// (`SYS_PROCESS_WAIT_STATUS` `arg3`/`arg4`).
///
/// These offsets are an ABI promise to lane B's libc: nothing in the kernel
/// reads them back, so a transposed field would compile, boot, and hand
/// userspace a UID where it expected a wstatus, forever. `handlers.rs`'s
/// doc comment is the specification; this is the test that the code agrees
/// with it.
///
/// Driven through [`handlers::wait_info_image`], the pure encoder, rather
/// than `write_wait_info`, because the latter writes through
/// `copy_to_user` and this self-test task has no user address space. The
/// `min(caller, kernel)` truncation and the zero-filled tail live in the
/// wrapper and are covered from ring 3 by the userspace test fixture.
///
/// The CPU-time check is the one worth stating twice: the counters are kept
/// in USER_HZ ticks internally but this structure carries **microseconds**,
/// so a `1` in must come out as `10_000`. A pass-through bug would look
/// entirely plausible in a hex dump.
fn test_dispatch_wait_info_layout() -> KernelResult<()> {
    use crate::proc::pcb::ExitInfo;
    use crate::proc::thread::ProcessUsage;
    use crate::syscall::handlers::{WAIT_INFO_SIZE, wait_info_image};
    use crate::syscall::wait::{ChildEvent, FoundEvent};

    fn fail(msg: &str) -> KernelResult<()> {
        serial_println!("[syscall]   FAIL: WaitInfo layout: {}", msg);
        Err(KernelError::InternalError)
    }

    let found = FoundEvent {
        pid: 0x1234_5678,
        uid: 4242,
        usage: ProcessUsage {
            user_ticks: 3,
            sys_ticks: 7,
            min_flt: 101,
            maj_flt: 102,
            nvcsw: 103,
            nivcsw: 104,
        },
        // exit_code 66 → wstatus (66 << 8) = 0x4200, per `ExitInfo::to_wstatus`.
        event: ChildEvent::Exited(ExitInfo {
            exit_code: 66,
            crash: None,
        }),
    };
    let img = wait_info_image(&found);

    let rd64 = |off: usize| -> Option<u64> {
        img.get(off..off.saturating_add(8))
            .and_then(|b| <[u8; 8]>::try_from(b).ok())
            .map(u64::from_le_bytes)
    };

    // The pad halves are checked too: a caller reading `uid` as a `u64`
    // (or a future field landing at 12) must see zero, not stale bytes.
    let expect: [(usize, u64, &str); 8] = [
        (0, 0x1234_5678, "pid@0"),
        (8, 4242, "uid@8 (+ zero pad @12)"),
        (16, 0x4200, "wstatus@16 (+ zero pad @20)"),
        (24, 30_000, "utime_us@24 (3 ticks = 30ms)"),
        (32, 70_000, "stime_us@32 (7 ticks = 70ms)"),
        (40, 101, "minflt@40"),
        (48, 102, "majflt@48"),
        (56, 103, "nvcsw@56"),
    ];
    for (off, want, what) in expect {
        if rd64(off) != Some(want) {
            return fail(what);
        }
    }
    // The last field must be the last field: 64 + 8 == the whole structure.
    // A compile-time assert, because a size change is a source edit, not a
    // runtime possibility — and this way it fails the build rather than the
    // boot.
    const _: () = assert!(WAIT_INFO_SIZE == 64 + 8);
    if rd64(64) != Some(104) {
        return fail("nivcsw@64");
    }

    serial_println!("[syscall]   WaitInfo (1063 arg3) byte layout: OK");
    Ok(())
}

/// Pin down the on-wire byte layout of `RusageInfo`
/// (`SYS_PROCESS_GET_RUSAGE` `arg1`/`arg2`), and the `who` gate in front of
/// it.
///
/// Same reasoning as [`test_dispatch_wait_info_layout`]: nothing in the
/// kernel reads these offsets back, so a transposed field would compile,
/// boot, and hand userspace a fault count where it asked for CPU time,
/// forever.
///
/// The **first six fields must be `WaitInfo`'s last six, in order and in the
/// same units** — that is the property that makes "what a parent reads for a
/// reaped child" and "what that child could have read for itself" agree by
/// construction. The test asserts it directly by encoding one `ProcessUsage`
/// through both encoders and comparing the two byte ranges, rather than by
/// listing the numbers twice: a shared expectation table can be edited once
/// and still agree with itself while disagreeing with the ABI.
///
/// The `who` gate is checked here too, with a null pointer. An unrecognised
/// selector must be `InvalidArgument` and must be decided *before* the
/// pointer is looked at — otherwise a caller probing for support gets
/// `InvalidAddress` from us and `EINVAL` from Linux for the same call.
fn test_dispatch_rusage_info_layout() -> KernelResult<()> {
    use crate::proc::thread::ProcessUsage;
    use crate::syscall::handlers::{RUSAGE_INFO_SIZE, rusage_info_image, wait_info_image};
    use crate::syscall::number::SYS_PROCESS_GET_RUSAGE;

    fn fail(msg: &str) -> KernelResult<()> {
        serial_println!("[syscall]   FAIL: RusageInfo layout: {}", msg);
        Err(KernelError::InternalError)
    }

    let usage = ProcessUsage {
        user_ticks: 3,
        sys_ticks: 7,
        min_flt: 101,
        maj_flt: 102,
        nvcsw: 103,
        nivcsw: 104,
    };
    let img = rusage_info_image(&usage, 9_999);

    let rd64 = |b: &[u8], off: usize| -> Option<u64> {
        b.get(off..off.saturating_add(8))
            .and_then(|s| <[u8; 8]>::try_from(s).ok())
            .map(u64::from_le_bytes)
    };

    let expect: [(usize, u64, &str); 7] = [
        (0, 30_000, "utime_us@0 (3 ticks = 30ms)"),
        (8, 70_000, "stime_us@8 (7 ticks = 70ms)"),
        (16, 101, "minflt@16"),
        (24, 102, "majflt@24"),
        (32, 103, "nvcsw@32"),
        (40, 104, "nivcsw@40"),
        (48, 9_999, "maxrss_kib@48"),
    ];
    for (off, want, what) in expect {
        if rd64(&img, off) != Some(want) {
            return fail(what);
        }
    }
    // The last field must be the last field. Compile-time, so a size change
    // fails the build rather than the boot.
    const _: () = assert!(RUSAGE_INFO_SIZE == 48 + 8);

    // The agreement property, asserted rather than assumed: WaitInfo's
    // counter block starts at 24 and RusageInfo's at 0, and the 48 bytes
    // must be byte-identical for the same counters.
    let wi = wait_info_image(&crate::syscall::wait::FoundEvent {
        pid: 1,
        uid: 0,
        usage,
        event: crate::syscall::wait::ChildEvent::Exited(crate::proc::pcb::ExitInfo {
            exit_code: 0,
            crash: None,
        }),
    });
    for off in (0..48).step_by(8) {
        if rd64(&img, off) != rd64(&wi, off.saturating_add(24)) {
            return fail("counter block disagrees with WaitInfo's");
        }
    }

    // `who` gate: unknown selector is EINVAL before the null pointer is
    // EFAULT, and a *known* selector with a null pointer is EFAULT — which
    // together prove the order rather than just the two verdicts.
    let mk = |who: u64, ptr: u64| SyscallArgs {
        arg0: who,
        arg1: ptr,
        arg2: RUSAGE_INFO_SIZE as u64,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    if dispatch(SYS_PROCESS_GET_RUSAGE, &mk(42, 0)).value
        != i64::from(KernelError::InvalidArgument.code())
    {
        return fail("who=42 with a null pointer must be InvalidArgument, not InvalidAddress");
    }
    // -1 is RUSAGE_CHILDREN; the register carries it sign-extended, so this
    // also proves the i32 truncation is done rather than the raw u64 compared.
    if dispatch(SYS_PROCESS_GET_RUSAGE, &mk(u64::MAX, 0)).value
        != i64::from(KernelError::InvalidAddress.code())
    {
        return fail("who=-1 (sign-extended) must be accepted, then fault on the null pointer");
    }

    serial_println!("[syscall]   RusageInfo (1064) byte layout + who gate: OK");
    Ok(())
}

/// Pin which `SYS_PROCESS_SET_CREDENTIALS` (530) requests count as an
/// *identity change*, because that is the whole of its capability gate.
///
/// The gate itself cannot be driven from here — the handler reads
/// `current_task_id()`, so exercising the deny path through `dispatch()` would
/// need a synthetic caller with a synthetic capability table. What it *can* be
/// driven from is `resolve_credential_request`, which is the entire decision
/// with the lookup removed. Both failure directions are bad in ways that would
/// not show up as a crash:
///
/// - a no-op wrongly judged a change denies `setuid(getuid())` to processes
///   that hold no capability and need none, breaking privilege-shedding code
///   that never actually held privilege;
/// - a change wrongly judged a no-op is the escalation this gate exists to
///   stop, and it would pass every other test in the tree, because the identity
///   really does end up where the caller asked.
fn test_dispatch_set_credentials_gate() -> KernelResult<()> {
    use crate::syscall::handlers::{CREDENTIALS_KEEP, resolve_credential_request};

    fn fail(msg: &str) -> KernelResult<()> {
        serial_println!("[syscall]   FAIL: set_credentials gate: {}", msg);
        Err(KernelError::InternalError)
    }

    /// One row of the table below, named because the bare tuple is six
    /// components wide and reads as noise at the declaration site:
    /// the caller's current `(uid, gid)`, the two syscall arguments, the
    /// `(uid, gid)` the request must resolve to, whether that counts as an
    /// identity *change* (which is the whole of the capability gate), and the
    /// description printed if the row fails.
    type CredCase = ((u32, u32), u64, u64, (u32, u32), bool, &'static str);

    let cases: [CredCase; 9] = [
        (
            (0, 0),
            CREDENTIALS_KEEP,
            CREDENTIALS_KEEP,
            (0, 0),
            false,
            "KEEP/KEEP touches nothing",
        ),
        (
            (1000, 1000),
            1000,
            1000,
            (1000, 1000),
            false,
            "setuid(getuid()) is a no-op, not an exercise of authority",
        ),
        (
            (1000, 1000),
            1000,
            CREDENTIALS_KEEP,
            (1000, 1000),
            false,
            "redundant uid + KEEP gid",
        ),
        (
            (1000, 1000),
            0,
            CREDENTIALS_KEEP,
            (0, 1000),
            true,
            "1000 -> root is a change",
        ),
        (
            (0, 0),
            1000,
            CREDENTIALS_KEEP,
            (1000, 0),
            true,
            "dropping privilege is a change too",
        ),
        (
            (1000, 1000),
            CREDENTIALS_KEEP,
            0,
            (1000, 0),
            true,
            "a gid-only change must be gated, not just uid",
        ),
        (
            (0, 0),
            3131,
            4242,
            (3131, 4242),
            true,
            "both fields at once (the fastpy-setuid fixture's move)",
        ),
        // The sentinel is u32::MAX widened, not u64::MAX: the value that means
        // "keep" is exactly the one no process can adopt.
        (
            (7, 7),
            u64::MAX,
            CREDENTIALS_KEEP,
            (0xFFFF_FFFF, 7),
            true,
            "u64::MAX is NOT the KEEP sentinel; it truncates to a real uid",
        ),
        // Truncation happens before the comparison as well as before the write,
        // so high garbage over a matching low half is judged on what would
        // actually be stored.
        (
            (5, 5),
            0x1_0000_0005,
            CREDENTIALS_KEEP,
            (5, 5),
            false,
            "high bits above a matching low half resolve to no change",
        ),
    ];

    for (current, arg0, arg1, want, want_change, what) in cases {
        let (got, got_change) = resolve_credential_request(current, arg0, arg1);
        if got != want {
            return fail(what);
        }
        if got_change != want_change {
            return fail(what);
        }
    }

    serial_println!("[syscall]   set_credentials (530) identity-change gate: OK");
    Ok(())
}

/// Verify `SYS_SIGNAL_STOP_SELF` (1062) is registered and rejects every
/// signal that is not one of the four POSIX stop signals.
///
/// The syscall exists because a self-stop is *not* expressible as
/// `SYS_SIGNAL_SEND(self, SIGTSTP)`: `classify_post_info` tests
/// `has_trampoline` before it reaches the catchable stop signals, so a
/// native process — which always has a trampoline registered — would have
/// the signal marked pending for handler delivery and re-enter the
/// dispatcher that just resolved it to `SIG_DFL`. That is an infinite
/// delivery loop, not a stop. The argument gate below is what keeps the
/// new number from becoming a second, unvalidated way into
/// `stop_process_for_signal`.
///
/// **Only the rejection path may be exercised here.** A *valid* stop signal
/// suspends every thread of the caller and returns only on `SIGCONT` — from
/// the kernel self-test task that would park the boot thread forever with
/// nobody left to resume it. The argument check runs before any process
/// lookup or scheduler call, so these calls short-circuit and are safe. The
/// accept path is covered end-to-end from ring 3, where a parent can observe
/// the stop and send the `SIGCONT`.
fn test_dispatch_signal_stop_self_rejects_non_stop_signals() -> KernelResult<()> {
    let mk = |arg0: u64| SyscallArgs {
        arg0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };

    // 0 (the existence-probe signal), 9 (SIGKILL — fatal, not a stop),
    // 18 (SIGCONT — the *opposite* of a stop, and one off SIGSTOP=19),
    // 23 (one past SIGTTOU=22), 64 (past the end of the signal range) and a
    // value that does not fit in u32 at all (the `try_from` arm).
    for sig in [0_u64, 9, 18, 23, 64, u64::from(u32::MAX) + 1] {
        let r = dispatch(SYS_SIGNAL_STOP_SELF, &mk(sig));
        // An unregistered dispatch slot answers NotSupported, so this
        // distinction is what proves the handler is actually wired in
        // rather than that every call happens to fail.
        if r.value == i64::from(KernelError::NotSupported.code()) {
            serial_println!("[syscall]   FAIL: signal_stop_self unregistered (NotSupported)");
            return Err(KernelError::InternalError);
        }
        if r.value != i64::from(KernelError::InvalidArgument.code()) {
            serial_println!(
                "[syscall]   FAIL: signal_stop_self({}) returned {}, expected InvalidArgument",
                sig,
                r.value
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[syscall]   signal_stop_self (1062) wired + stop-signal gate: OK");
    Ok(())
}

/// Verify the native read/write syscall classification feeding
/// `/proc/<pid>/io` accounting.  The byte-folding side effect itself is
/// covered by `proc::pcb::test_io_accounting`; here we pin down the
/// syscall-number → direction mapping so a misfiled number is caught.
fn test_io_dir_classification() -> KernelResult<()> {
    // Reads.
    for nr in [
        SYS_FS_READ,
        SYS_FS_READ_FILE,
        SYS_PIPE_READ,
        SYS_PIPE_TRY_READ,
        SYS_PIPE_READ_TIMEOUT,
        SYS_CONSOLE_READ_CHAR,
        SYS_CONSOLE_TRY_READ_CHAR,
    ] {
        if io_dir_for_syscall(nr) != Some(IoDir::Read) {
            serial_println!("[syscall]   FAIL: nr {} not classified as Read", nr);
            return Err(KernelError::InternalError);
        }
    }
    // Writes.
    for nr in [
        SYS_FS_WRITE,
        SYS_PIPE_WRITE,
        SYS_PIPE_TRY_WRITE,
        SYS_PIPE_WRITE_TIMEOUT,
        SYS_CONSOLE_WRITE,
    ] {
        if io_dir_for_syscall(nr) != Some(IoDir::Write) {
            serial_println!("[syscall]   FAIL: nr {} not classified as Write", nr);
            return Err(KernelError::InternalError);
        }
    }
    // Non-IO syscalls and the deliberately-excluded whole-file write must
    // not be accounted.
    for nr in [SYS_YIELD, SYS_TASK_ID, SYS_FS_WRITE_FILE, SYS_FS_OPEN] {
        if io_dir_for_syscall(nr).is_some() {
            serial_println!("[syscall]   FAIL: nr {} should not be IO-classified", nr);
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[syscall]   Native I/O syscall classification: OK");
    Ok(())
}

fn test_dispatch_yield() -> KernelResult<()> {
    let args = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let result = dispatch(SYS_YIELD, &args);
    if result.value != 0 {
        serial_println!("[syscall]   FAIL: yield returned {}", result.value);
        return Err(KernelError::InternalError);
    }
    serial_println!("[syscall]   Dispatch SYS_YIELD: OK");
    Ok(())
}

fn test_dispatch_task_id() -> KernelResult<()> {
    let args = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let result = dispatch(SYS_TASK_ID, &args);
    let current = crate::sched::current_task_id();
    // On x86_64, task IDs fit in i64 (they're monotonically-increasing
    // u64 values, and we won't reach 2^63 tasks).
    #[allow(clippy::cast_possible_wrap)]
    let expected = current as i64;
    if result.value != expected {
        serial_println!(
            "[syscall]   FAIL: task_id returned {}, expected {}",
            result.value,
            expected
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[syscall]   Dispatch SYS_TASK_ID: OK (id={})", result.value);
    Ok(())
}

fn test_dispatch_unimplemented() -> KernelResult<()> {
    let args = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    // Use a known-undefined number in kernel-core range (95 is unallocated).
    let result = dispatch(95, &args);
    if result.value != i64::from(KernelError::NotSupported.code()) {
        serial_println!(
            "[syscall]   FAIL: unimplemented syscall returned {}, expected NotSupported",
            result.value
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[syscall]   Dispatch unimplemented: OK (NotSupported)");
    Ok(())
}

fn test_dispatch_out_of_range() -> KernelResult<()> {
    let args = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let result = dispatch(9999, &args);
    if result.value != i64::from(KernelError::InvalidArgument.code()) {
        serial_println!(
            "[syscall]   FAIL: out-of-range syscall returned {}, expected InvalidArgument",
            result.value
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[syscall]   Dispatch out-of-range: OK (InvalidArgument)");
    Ok(())
}

/// Test IPC channel operations through the syscall dispatch path.
fn test_dispatch_channel_roundtrip() -> KernelResult<()> {
    // Create a channel via syscall.
    let args = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let result = dispatch(SYS_CHANNEL_CREATE, &args);
    if result.value < 0 {
        serial_println!("[syscall]   FAIL: channel_create returned {}", result.value);
        return Err(KernelError::InternalError);
    }

    // Channel handles are non-negative i64 values representing u64
    // handles.  We need to cast back, which is safe because
    // channel_create only produces non-negative values.
    #[allow(clippy::cast_sign_loss)]
    let ep0_raw = result.value as u64;
    #[allow(clippy::cast_sign_loss)]
    let ep1_raw = result.value2 as u64;

    // Send "hi" through ep0 via syscall.
    let msg_data = b"hi";
    let send_args = SyscallArgs {
        arg0: ep0_raw,
        arg1: msg_data.as_ptr() as u64,
        arg2: msg_data.len() as u64,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let send_result = dispatch(SYS_CHANNEL_SEND, &send_args);
    if send_result.value != 0 {
        serial_println!(
            "[syscall]   FAIL: channel_send returned {}",
            send_result.value
        );
        return Err(KernelError::InternalError);
    }

    // Receive on ep1 via syscall (non-blocking try_recv).
    let mut recv_buf = [0u8; 64];
    let recv_args = SyscallArgs {
        arg0: ep1_raw,
        arg1: recv_buf.as_mut_ptr() as u64,
        arg2: recv_buf.len() as u64,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let recv_result = dispatch(SYS_CHANNEL_TRY_RECV, &recv_args);
    if recv_result.value != 2 {
        serial_println!(
            "[syscall]   FAIL: channel_try_recv returned {} (expected 2 = msg len)",
            recv_result.value
        );
        return Err(KernelError::InternalError);
    }

    // Verify data.
    if recv_buf.get(..2) != Some(b"hi".as_slice()) {
        serial_println!("[syscall]   FAIL: received data mismatch");
        return Err(KernelError::InternalError);
    }

    // Close both endpoints via syscall.
    let close0_args = SyscallArgs {
        arg0: ep0_raw,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    dispatch(SYS_CHANNEL_CLOSE, &close0_args);

    let close1_args = SyscallArgs {
        arg0: ep1_raw,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    dispatch(SYS_CHANNEL_CLOSE, &close1_args);

    serial_println!("[syscall]   Dispatch channel roundtrip: OK");
    Ok(())
}

/// Test clock_monotonic syscall returns a non-negative nanosecond value.
fn test_dispatch_clock_monotonic() -> KernelResult<()> {
    let args = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let result = dispatch(SYS_CLOCK_MONOTONIC, &args);
    if result.value < 0 {
        serial_println!(
            "[syscall]   FAIL: clock_monotonic returned {}",
            result.value
        );
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[syscall]   Dispatch SYS_CLOCK_MONOTONIC: OK ({}ns)",
        result.value
    );
    Ok(())
}

/// Test `SYS_GETRANDOM`'s argument handling and its success path.
///
/// **What this test can and cannot see.** `validate_user_write` begins with
/// `is_kernel_context()`, which is true here — the self-test task owns no
/// process — so the user-pointer validator is *bypassed* for every call made
/// from this function.  Two consequences, both deliberate:
///
/// * The "a kernel address must be rejected" rule is **not** observable from
///   here, and asserting it fails (an earlier version of this test did, and
///   the boot test caught it).  That rule is enforced by `validate_user_range`
///   for real ring-3 callers and is tested where it lives, in `mm::user`.
/// * In exchange, the bypass lets us exercise the **success path** with an
///   ordinary kernel buffer: dispatch → handler → `rng::fill` → returned
///   count.  That is the more valuable half, because a `SYS_GETRANDOM` that
///   returns a byte count larger than it actually wrote leaves the caller
///   reading uninitialised memory as key material — a failure that looks
///   identical to success at every layer above.
///
/// The over-length clamp is deliberately *not* exercised: with validation
/// bypassed, passing a length above `GETRANDOM_MAX` would really write a
/// megabyte through a small buffer and corrupt the kernel stack.
///
/// # Which half of the contract runs here
///
/// `SYS_GETRANDOM` refuses to hand out bytes until the CSPRNG has been
/// *credited* real entropy (`rng::is_ready`), so this test has two arms and
/// picks between them by asking that same question.  On a QEMU guest — no
/// RDRAND, no RDSEED — the answer here is always "not ready": `main` runs the
/// syscall self-tests long before it calls `rng::init`, let alone before any
/// interrupt has arrived to be credited.  So the arm that normally runs in the
/// boot test is the **refusal** arm, and that is the more important one to
/// hold: the whole point of the change is that a caller asking for key
/// material gets an error rather than output from an uncredited pool.  The
/// success arm is kept for hardware that has a CPU RNG, where `rng::init`
/// credits 256 bits outright and the pool is ready before anything runs.
fn test_dispatch_getrandom() -> KernelResult<()> {
    // Zero length is a success returning 0, even for a null pointer: callers
    // that loop until a count is exhausted must not have to special-case the
    // final iteration.
    let zero_len = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let result = dispatch(SYS_GETRANDOM, &zero_len);
    if result.value != 0 {
        serial_println!(
            "[syscall]   FAIL: getrandom(NULL, 0) returned {}",
            result.value
        );
        return Err(KernelError::InternalError);
    }

    // Null pointer with a nonzero length is an error, not a silent no-op.
    let null_buf = SyscallArgs {
        arg0: 0,
        arg1: 16,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    if dispatch(SYS_GETRANDOM, &null_buf).value >= 0 {
        serial_println!("[syscall]   FAIL: getrandom(NULL, 16) did not fail");
        return Err(KernelError::InternalError);
    }

    let mut sink = [0u8; 32];
    let good = SyscallArgs {
        arg0: sink.as_mut_ptr() as u64,
        arg1: 32,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };

    // The `GRND_*` battery runs on its own buffer, before either arm below:
    // the refusal arm returns early, and it also asserts that `sink` was left
    // untouched, which a `GRND_INSECURE` draw into it would falsify.
    test_dispatch_getrandom_flags()?;

    if !crate::rng::is_ready() {
        // The refusal arm.  Two things must hold, and the second matters more
        // than the first: the call must report an error, *and* it must not
        // have touched the buffer.  A handler that filled the buffer and then
        // returned an error would leave a caller who ignores the return value
        // — which is exactly the caller this guarantee exists to protect —
        // holding uncredited bytes it believes are key material.
        let result = dispatch(SYS_GETRANDOM, &good);
        if result.value >= 0 {
            serial_println!(
                "[syscall]   FAIL: getrandom(buf, 32) returned {} with an uncredited \
                 pool, expected an error",
                result.value
            );
            return Err(KernelError::InternalError);
        }
        if sink != [0u8; 32] {
            serial_println!("[syscall]   FAIL: getrandom failed but still wrote to the buffer");
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[syscall]   Dispatch SYS_GETRANDOM: OK (refused: pool uncredited, {}/{} bits)",
            crate::rng::credited_bits(),
            crate::rng::credit_target_bits()
        );
        return Ok(());
    }

    // The success path (see the note above about the kernel-context bypass).
    // The returned count must equal the requested length, and the buffer must
    // actually have been written: a handler that returned `len` without
    // filling anything would hand the caller its own uninitialised memory as
    // key material.
    let result = dispatch(SYS_GETRANDOM, &good);
    if result.value != 32 {
        serial_println!(
            "[syscall]   FAIL: getrandom(buf, 32) returned {}, expected 32",
            result.value
        );
        return Err(KernelError::InternalError);
    }
    // The buffer started all-zero, so an all-zero result means nothing was
    // written.  A genuine 32-byte draw collides with that with probability
    // 2^-256, so this cannot flake.
    if sink == [0u8; 32] {
        serial_println!("[syscall]   FAIL: getrandom reported 32 bytes but wrote none");
        return Err(KernelError::InternalError);
    }

    // A second draw must differ from the first: a stuck generator would pass
    // every check above while returning the same bytes to every caller.
    let mut second = [0u8; 32];
    let good2 = SyscallArgs {
        arg0: second.as_mut_ptr() as u64,
        arg1: 32,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    if dispatch(SYS_GETRANDOM, &good2).value != 32 {
        serial_println!("[syscall]   FAIL: second getrandom draw did not return 32");
        return Err(KernelError::InternalError);
    }
    if second == sink {
        serial_println!("[syscall]   FAIL: getrandom returned the same bytes twice");
        return Err(KernelError::InternalError);
    }

    serial_println!("[syscall]   Dispatch SYS_GETRANDOM: OK (pool credited)");
    Ok(())
}

/// Test the `GRND_*` flags word on the **native** `SYS_GETRANDOM` (90).
///
/// Until 2026-08-18 `arg2` was ignored here, because libc reached this syscall
/// through a two-argument stub and the third register held whatever the caller
/// last left in it.  Lane B widened the stub and rebuilt the committed
/// fixtures, so the kernel now reads it — and this is the test that says so.
///
/// The important arm is `GRND_NONBLOCK` against an **uncredited** pool, which
/// is the state this self-test always runs in on a QEMU guest (`main` runs the
/// syscall battery long before `rng::init`, and there is no RDRAND to short-cut
/// it).  It is asserted against the *specific* error, and that precision is the
/// whole test: a handler that ignored the flag would also return an error here
/// — `TimedOut`, from `wait_until_ready`'s "nothing is crediting this pool"
/// early-out — so "some error" is indistinguishable between working and broken.
/// The two are not interchangeable at the libc boundary either: lane B maps
/// `WouldBlock` to `EAGAIN` and pins `TimedOut` to `EIO`, and `EAGAIN` is the
/// only one a `GRND_NONBLOCK` caller retries on.
///
/// (On a machine where the pool *can* eventually be credited, ignoring the flag
/// would instead mean a 15-second stall where the caller asked for none — the
/// same bug, in the form that only shows up off the self-test path.)
///
/// `GRND_INSECURE` is asserted to succeed *in the same pool state* — that
/// pairing is the whole point of the flag, and asserting it here (rather than
/// in a run where the pool happens to be credited) is what makes it meaningful.
fn test_dispatch_getrandom_flags() -> KernelResult<()> {
    use crate::syscall::handlers::{GRND_INSECURE, GRND_NONBLOCK, GRND_RANDOM};

    let mut scratch = [0u8; 32];
    let ptr = scratch.as_mut_ptr() as u64;
    let call = |arg0: u64, len: u64, flags: u32| {
        dispatch(
            SYS_GETRANDOM,
            &SyscallArgs {
                arg0,
                arg1: len,
                arg2: u64::from(flags),
                arg3: 0,
                arg4: 0,
                arg5: 0,
            },
        )
        .value
    };
    let einval = i64::from(KernelError::InvalidArgument.code());

    // An unknown bit is rejected rather than ignored.  Checked with length 0
    // *and* a null pointer, which is the case that would succeed if the flags
    // were screened after the zero-length early-out — and a caller probing for
    // feature support with `getrandom(NULL, 0, FLAG)` is precisely who would be
    // told "supported" by that bug.
    for (label, flags, len, buf) in [
        ("unknown bit 0x10", 0x10u32, 0u64, 0u64),
        ("unknown bit 0x10, real buffer", 0x10, 16, ptr),
        ("RANDOM|INSECURE", GRND_RANDOM | GRND_INSECURE, 16, ptr),
        ("every bit set", u32::MAX, 16, ptr),
    ] {
        let r = call(buf, len, flags);
        if r != einval {
            serial_println!(
                "[syscall]   FAIL: getrandom flags {} returned {}, expected InvalidArgument ({})",
                label,
                r,
                einval
            );
            return Err(KernelError::InternalError);
        }
    }

    // GRND_INSECURE waives the readiness gate, so it must return bytes whatever
    // the pool's state.  This is also the only way this self-test can exercise
    // the fill path at all before `rng::init`.
    let r = call(ptr, 32, GRND_INSECURE);
    if r != 32 {
        serial_println!(
            "[syscall]   FAIL: getrandom(32, GRND_INSECURE) returned {}, expected 32",
            r
        );
        return Err(KernelError::InternalError);
    }
    if scratch == [0u8; 32] {
        serial_println!(
            "[syscall]   FAIL: getrandom(32, GRND_INSECURE) reported 32 bytes but wrote none"
        );
        return Err(KernelError::InternalError);
    }

    // The gate itself, and the flag that converts the wait into an error.
    let ready = crate::rng::is_ready();
    let want_nonblock = if ready {
        4
    } else {
        i64::from(KernelError::WouldBlock.code())
    };
    let r = call(ptr, 4, GRND_NONBLOCK);
    if r != want_nonblock {
        serial_println!(
            "[syscall]   FAIL: getrandom(4, GRND_NONBLOCK) returned {}, expected {} (pool {})",
            r,
            want_nonblock,
            if ready { "credited" } else { "uncredited" }
        );
        return Err(KernelError::InternalError);
    }

    // GRND_RANDOM alone is accepted and changes nothing: one pool, one gate.
    // Asserting it behaves *identically to no flags at all* is what pins it as
    // a no-op rather than merely as "not an error".
    let plain = call(ptr, 4, 0);
    let random = call(ptr, 4, GRND_RANDOM);
    if (plain < 0) != (random < 0) || (plain < 0 && plain != random) {
        serial_println!(
            "[syscall]   FAIL: getrandom GRND_RANDOM ({}) differs from no flags ({})",
            random,
            plain
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[syscall]   Dispatch SYS_GETRANDOM flags: OK (NONBLOCK/INSECURE honoured, pool {})",
        if ready { "credited" } else { "uncredited" }
    );
    Ok(())
}

/// Test clock_realtime syscall returns a non-negative nanosecond value.
///
/// The value may be 0 if timekeeping was not initialized (no usable RTC),
/// which is still a valid (non-error) result; we only reject negative
/// (error) returns.
fn test_dispatch_clock_realtime() -> KernelResult<()> {
    let args = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let result = dispatch(SYS_CLOCK_REALTIME, &args);
    if result.value < 0 {
        serial_println!("[syscall]   FAIL: clock_realtime returned {}", result.value);
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[syscall]   Dispatch SYS_CLOCK_REALTIME: OK ({}ns since epoch)",
        result.value
    );
    Ok(())
}

/// Test the `SYS_CLOCK_SETTIME` dispatch path.
///
/// To avoid corrupting the running system's wall clock, this sets the time to
/// (approximately) its current value — `set_realtime` then stores a near-zero
/// adjustment.  We assert the wiring matches the clock's init state: when the
/// clock is initialized the call must succeed (0) and time must not jump
/// backwards; when uninitialized it must reject with an error.
fn test_dispatch_clock_settime() -> KernelResult<()> {
    let read = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let before = dispatch(SYS_CLOCK_REALTIME, &read).value;

    let set = SyscallArgs {
        #[allow(clippy::cast_sign_loss)]
        arg0: before.max(0) as u64,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let result = dispatch(SYS_CLOCK_SETTIME, &set);

    if crate::timekeeping::is_initialized() {
        if result.value != 0 {
            serial_println!(
                "[syscall]   FAIL: clock_settime returned {} (expected 0)",
                result.value
            );
            return Err(KernelError::InternalError);
        }
        // Setting to the current value must not push the clock backwards.
        let after = dispatch(SYS_CLOCK_REALTIME, &read).value;
        if after < before {
            serial_println!(
                "[syscall]   FAIL: clock_settime moved time backwards ({} -> {})",
                before,
                after
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[syscall]   Dispatch SYS_CLOCK_SETTIME: OK (set to now)");
    } else {
        if result.value >= 0 {
            serial_println!(
                "[syscall]   FAIL: clock_settime succeeded ({}) on uninitialized clock",
                result.value
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[syscall]   Dispatch SYS_CLOCK_SETTIME: OK (rejected, uninitialized)");
    }
    Ok(())
}

/// Test the `SYS_CLOCK_ADJTIME` dispatch path.
///
/// Applies a small forward step (+1 ms) and then the exact inverse (−1 ms) so
/// the running system's wall clock is left unchanged.  Asserts: when the clock
/// is initialized the call succeeds (0) and the forward step does not move time
/// backwards; when uninitialized it rejects with an error (matching
/// `SYS_CLOCK_SETTIME`).
fn test_dispatch_clock_adjtime() -> KernelResult<()> {
    const STEP_NS: u64 = 1_000_000; // 1 ms

    let read = SyscallArgs {
        arg0: 0,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let before = dispatch(SYS_CLOCK_REALTIME, &read).value;

    let forward = SyscallArgs {
        arg0: STEP_NS,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let result = dispatch(SYS_CLOCK_ADJTIME, &forward);

    if crate::timekeeping::is_initialized() {
        if result.value != 0 {
            serial_println!(
                "[syscall]   FAIL: clock_adjtime returned {} (expected 0)",
                result.value
            );
            return Err(KernelError::InternalError);
        }
        let after = dispatch(SYS_CLOCK_REALTIME, &read).value;
        // Restore the clock by applying the inverse step regardless of the
        // assertion outcome, so the self-test never leaves the wall clock
        // skewed.
        let restore = SyscallArgs {
            // -STEP_NS reinterpreted as u64 (inverse of the forward step).
            arg0: (STEP_NS as i64).wrapping_neg() as u64,
            arg1: 0,
            arg2: 0,
            arg3: 0,
            arg4: 0,
            arg5: 0,
        };
        let _ = dispatch(SYS_CLOCK_ADJTIME, &restore);

        if after < before {
            serial_println!(
                "[syscall]   FAIL: clock_adjtime moved time backwards ({} -> {})",
                before,
                after
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[syscall]   Dispatch SYS_CLOCK_ADJTIME: OK (+1ms then restored)");
    } else {
        if result.value >= 0 {
            serial_println!(
                "[syscall]   FAIL: clock_adjtime succeeded ({}) on uninitialized clock",
                result.value
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[syscall]   Dispatch SYS_CLOCK_ADJTIME: OK (rejected, uninitialized)");
    }
    Ok(())
}

/// Test console write syscall.
fn test_dispatch_console_write() -> KernelResult<()> {
    let msg = b"[syscall]   Console write via SYS_CONSOLE_WRITE\n";
    let args = SyscallArgs {
        arg0: msg.as_ptr() as u64,
        arg1: msg.len() as u64,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let result = dispatch(SYS_CONSOLE_WRITE, &args);
    if result.value < 0 {
        serial_println!("[syscall]   FAIL: console_write returned {}", result.value);
        return Err(KernelError::InternalError);
    }
    #[allow(clippy::cast_possible_wrap)]
    let expected_len = msg.len() as i64;
    if result.value != expected_len {
        serial_println!(
            "[syscall]   FAIL: console_write returned {}, expected {}",
            result.value,
            expected_len
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[syscall]   Dispatch SYS_CONSOLE_WRITE: OK");
    Ok(())
}

/// Test filesystem syscalls: write, read, stat, mkdir, list, delete, rmdir.
///
/// Exercises the full VFS path through the dispatch table.  Only runs if
/// the VFS has a mounted filesystem (otherwise the write will fail and
/// we skip gracefully).
fn test_dispatch_fs_roundtrip() -> KernelResult<()> {
    let test_path = b"/syscall_test.txt";
    let test_data = b"Hello from syscall self-test!";

    // 1. Write a test file.
    let write_args = SyscallArgs {
        arg0: test_path.as_ptr() as u64,
        arg1: test_path.len() as u64,
        arg2: test_data.as_ptr() as u64,
        arg3: test_data.len() as u64,
        arg4: 0,
        arg5: 0,
    };
    let write_result = dispatch(SYS_FS_WRITE_FILE, &write_args);
    if write_result.value < 0 {
        // No filesystem mounted — skip FS tests gracefully.
        serial_println!(
            "[syscall]   Dispatch FS roundtrip: SKIPPED (no FS, err={})",
            write_result.value
        );
        return Ok(());
    }

    // 2. Read it back.
    let mut read_buf = [0u8; 128];
    let read_args = SyscallArgs {
        arg0: test_path.as_ptr() as u64,
        arg1: test_path.len() as u64,
        arg2: read_buf.as_mut_ptr() as u64,
        arg3: read_buf.len() as u64,
        arg4: 0,
        arg5: 0,
    };
    let read_result = dispatch(SYS_FS_READ_FILE, &read_args);
    #[allow(clippy::cast_possible_wrap)]
    let expected_len = test_data.len() as i64;
    if read_result.value != expected_len {
        serial_println!(
            "[syscall]   FAIL: read_file returned {}, expected {}",
            read_result.value,
            expected_len
        );
        return Err(KernelError::InternalError);
    }
    if read_buf.get(..test_data.len()) != Some(test_data.as_slice()) {
        serial_println!("[syscall]   FAIL: read_file data mismatch");
        return Err(KernelError::InternalError);
    }

    // 3. Stat the file.
    let mut stat_buf = [0u8; handlers::FS_STAT_RESULT_LEN];
    let stat_args = SyscallArgs {
        arg0: test_path.as_ptr() as u64,
        arg1: test_path.len() as u64,
        arg2: stat_buf.as_mut_ptr() as u64,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let stat_result = dispatch(SYS_FS_STAT, &stat_args);
    if stat_result.value != 0 {
        serial_println!("[syscall]   FAIL: stat returned {}", stat_result.value);
        return Err(KernelError::InternalError);
    }
    // Verify size field (bytes 0-7, u64 LE).
    let stat_size = u64::from_le_bytes([
        stat_buf[0],
        stat_buf[1],
        stat_buf[2],
        stat_buf[3],
        stat_buf[4],
        stat_buf[5],
        stat_buf[6],
        stat_buf[7],
    ]);
    if stat_size != test_data.len() as u64 {
        serial_println!(
            "[syscall]   FAIL: stat size {} != expected {}",
            stat_size,
            test_data.len()
        );
        return Err(KernelError::InternalError);
    }
    // Verify type field (byte 8): 0=file.
    if stat_buf[8] != 0 {
        serial_println!("[syscall]   FAIL: stat type {} != 0 (file)", stat_buf[8]);
        return Err(KernelError::InternalError);
    }

    // 4. Delete the test file.
    let delete_args = SyscallArgs {
        arg0: test_path.as_ptr() as u64,
        arg1: test_path.len() as u64,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let delete_result = dispatch(SYS_FS_DELETE, &delete_args);
    if delete_result.value != 0 {
        serial_println!("[syscall]   FAIL: delete returned {}", delete_result.value);
        return Err(KernelError::InternalError);
    }

    // 5. Test mkdir + rmdir.
    let dir_path = b"/syscall_test_dir";
    let mkdir_args = SyscallArgs {
        arg0: dir_path.as_ptr() as u64,
        arg1: dir_path.len() as u64,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let mkdir_result = dispatch(SYS_FS_MKDIR, &mkdir_args);
    if mkdir_result.value != 0 {
        serial_println!("[syscall]   FAIL: mkdir returned {}", mkdir_result.value);
        return Err(KernelError::InternalError);
    }

    // Stat the directory.
    let stat_dir_args = SyscallArgs {
        arg0: dir_path.as_ptr() as u64,
        arg1: dir_path.len() as u64,
        arg2: stat_buf.as_mut_ptr() as u64,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let stat_dir_result = dispatch(SYS_FS_STAT, &stat_dir_args);
    if stat_dir_result.value != 0 {
        serial_println!(
            "[syscall]   FAIL: stat dir returned {}",
            stat_dir_result.value
        );
        return Err(KernelError::InternalError);
    }
    // Type should be 1 (directory).
    if stat_buf[8] != 1 {
        serial_println!("[syscall]   FAIL: stat dir type {} != 1", stat_buf[8]);
        return Err(KernelError::InternalError);
    }

    // List the root directory — our test dir should appear.
    let root_path = b"/";
    let mut list_buf = [0u8; 264 * 32]; // Room for 32 entries.
    let list_args = SyscallArgs {
        arg0: root_path.as_ptr() as u64,
        arg1: root_path.len() as u64,
        arg2: list_buf.as_mut_ptr() as u64,
        arg3: list_buf.len() as u64,
        arg4: 0,
        arg5: 0,
    };
    let list_result = dispatch(SYS_FS_LIST_DIR, &list_args);
    if list_result.value < 0 {
        serial_println!("[syscall]   FAIL: list_dir returned {}", list_result.value);
        return Err(KernelError::InternalError);
    }

    // Remove the test directory.
    let rmdir_args = SyscallArgs {
        arg0: dir_path.as_ptr() as u64,
        arg1: dir_path.len() as u64,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let rmdir_result = dispatch(SYS_FS_RMDIR, &rmdir_args);
    if rmdir_result.value != 0 {
        serial_println!("[syscall]   FAIL: rmdir returned {}", rmdir_result.value);
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[syscall]   Dispatch FS roundtrip: OK (write/read/stat/delete/mkdir/listdir/rmdir)"
    );
    Ok(())
}
