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

use super::number::{
    MAX_SYSCALL_NR, SYS_CHANNEL_CLOSE, SYS_CHANNEL_CREATE, SYS_CHANNEL_RECV,
    SYS_CHANNEL_SEND, SYS_CHANNEL_TRY_RECV, SYS_CONSOLE_READ_CHAR,
    SYS_CONSOLE_TRY_READ_CHAR,
    SYS_CONSOLE_WRITE, SYS_CP_CLOSE, SYS_CP_CREATE, SYS_CP_NOTIFY,
    SYS_CP_REGISTER, SYS_CP_TRY_WAIT, SYS_CP_UNREGISTER, SYS_CP_WAIT,
    SYS_CLOCK_MONOTONIC,
    SYS_CLOCK_REALTIME,
    SYS_CLOCK_SETTIME,
    SYS_CLOCK_ADJTIME,
    SYS_DEBUG_PRINT, SYS_LOG_READ,
    SYS_CHANNEL_SEND_BLOCKING, SYS_CHANNEL_SEND_TIMEOUT,
    SYS_EVENTFD_CLOSE, SYS_EVENTFD_CREATE, SYS_EVENTFD_HAS_VALUE,
    SYS_TIMER_CANCEL, SYS_TIMER_CREATE,
    SYS_EVENTFD_READ, SYS_EVENTFD_READ_TIMEOUT, SYS_EVENTFD_TRY_READ,
    SYS_EVENTFD_WRITE, SYS_EVENTFD_WRITE_TIMEOUT, SYS_EXIT,
    SYS_FS_DELETE, SYS_FS_LIST_DIR, SYS_FS_MKDIR, SYS_FS_READ_FILE,
    SYS_FS_RMDIR, SYS_FS_STAT, SYS_FS_LINK, SYS_FS_STATVFS, SYS_FS_FLOCK,
    SYS_FS_FUNLOCK, SYS_FS_SYNC,
    SYS_FS_COPY, SYS_FS_APPEND, SYS_FS_FTRUNCATE, SYS_FS_DUP, SYS_FS_HANDLE_PATH,
    SYS_FS_READDIR_AT, SYS_FS_TMPFILE,
    SYS_FS_FALLOCATE, SYS_FS_SEEK_DATA, SYS_FS_SEEK_HOLE,
    SYS_FS_MOUNT, SYS_FS_UMOUNT, SYS_FS_FORMAT, SYS_FS_CHECK, SYS_FS_TRIM,
    SYS_FS_WRITE_FILE,
    SYS_FS_OPEN, SYS_FS_CLOSE, SYS_FS_READ, SYS_FS_WRITE,
    SYS_FS_SEEK, SYS_FS_TRUNCATE, SYS_FS_RENAME, SYS_FS_FSTAT,
    SYS_FS_TRASH, SYS_FS_TRASH_LIST, SYS_FS_TRASH_RESTORE, SYS_FS_TRASH_EMPTY,
    SYS_FS_WATCH_CREATE, SYS_FS_WATCH_READ, SYS_FS_WATCH_CLOSE,
    SYS_FS_JOURNAL_CURSOR, SYS_FS_JOURNAL_READ, SYS_FS_JOURNAL_FLUSH,
    SYS_FS_METADATA, SYS_FS_SET_ATTR, SYS_FS_SET_OWNER, SYS_FS_SET_PERMS,
    SYS_FS_SET_TIMES, SYS_FS_GET_XATTR, SYS_FS_SET_XATTR, SYS_FS_REMOVE_XATTR,
    SYS_FS_LIST_XATTRS,
    SYS_FS_SYMLINK, SYS_FS_READLINK, SYS_FS_LSTAT,
    SYS_FUTEX_LOCK_PI, SYS_FUTEX_UNLOCK_PI,
    SYS_FUTEX_TRYLOCK_PI, SYS_FUTEX_LOCK_PI_TIMEOUT,
    SYS_FUTEX_WAIT_REQUEUE_PI, SYS_FUTEX_CMP_REQUEUE_PI,
    SYS_FUTEX_REQUEUE,
    SYS_FUTEX_WAIT, SYS_FUTEX_WAIT_TIMEOUT, SYS_FUTEX_WAKE,
    SYS_IRQ_REGISTER, SYS_IRQ_RELEASE,
    SYS_IRQ_WAIT, SYS_PIPE_CLOSE, SYS_PIPE_CREATE, SYS_PIPE_POLL,
    SYS_PIPE_READ, SYS_PIPE_READABLE_BYTES,
    SYS_PIPE_TRY_READ, SYS_PIPE_TRY_WRITE, SYS_PIPE_WRITE,
    SYS_PORT_READ, SYS_PORT_WRITE,
    SYS_DMA_ALLOC, SYS_DMA_FREE,
    SYS_DMA_DOMAIN_CREATE, SYS_DMA_DOMAIN_DESTROY,
    SYS_DMA_MAP, SYS_DMA_UNMAP,
    SYS_DMA_ATTACH, SYS_DMA_DETACH,
    SYS_CPU_COUNT,
    SYS_PHYS_PAGES_TOTAL, SYS_PHYS_PAGES_AVAIL,
    SYS_LOADAVG,
    SYS_CPU_TIMES,
    SYS_SCHED_GET_PROFILE, SYS_SCHED_GET_TIMESLICE, SYS_SCHED_RECONFIGURE,
    SYS_SCHED_SET_PROFILE, SYS_SCHED_SET_TIMESLICE,
    SYS_SYSCTL_GET, SYS_SYSCTL_SET,
    SYS_MM_SET_PROFILE, SYS_MM_GET_PROFILE,
    SYS_SYSTEM_SET_PROFILE,
    SYS_CAP_QUERY, SYS_CAP_REQUEST, SYS_CAP_REQUEST_STATUS, SYS_CAP_REQUEST_CANCEL,
    SYS_MMAP, SYS_MUNMAP, SYS_MPROTECT, SYS_PROCESS_ID,
    SYS_NOTIFY_READY, SYS_PROCESS_IS_READY,
    SYS_PROCESS_CRASH_INFO,
    SYS_PROCESS_GET_ARGS, SYS_PROCESS_GET_INITIAL_FDS,
    SYS_PROCESS_PARENT_ID,
    SYS_PROCESS_COUNT,
    SYS_SIGNAL_REGISTER,
    SYS_SIGNAL_SEND,
    SYS_SIGNAL_MASK,
    SYS_SIGNAL_PENDING,
    SYS_PROCESS_KILL, SYS_PROCESS_SPAWN, SYS_PROCESS_SPAWN_EX,
    SYS_PROCESS_TRY_WAIT, SYS_PROCESS_WAIT,
    SYS_SET_EXCEPTION_HANDLER,
    SYS_SHM_CLOSE, SYS_SHM_CREATE, SYS_SHM_MAP, SYS_SHM_SIZE, SYS_SHM_UNMAP,
    SYS_SLEEP, SYS_TASK_ID,
    SYS_SOCKETPAIR_CREATE, SYS_SOCKETPAIR_SEND, SYS_SOCKETPAIR_RECV,
    SYS_SOCKETPAIR_TRY_SEND, SYS_SOCKETPAIR_TRY_RECV, SYS_SOCKETPAIR_CLOSE,
    SYS_SOCKETPAIR_SEND_TIMEOUT, SYS_SOCKETPAIR_RECV_TIMEOUT,
    SYS_SOCKETPAIR_POLL, SYS_SOCKETPAIR_READABLE_BYTES, SYS_SOCKETPAIR_SHUTDOWN,
    SYS_TCP_ACCEPT, SYS_TCP_ABORT, SYS_TCP_BIND, SYS_TCP_CLOSE, SYS_TCP_CLOSE_LISTENER,
    SYS_TCP_PEER_ADDR,
    SYS_TCP_CONNECT, SYS_TCP_RECV, SYS_TCP_SEND,
    SYS_THREAD_CREATE, SYS_THREAD_EXIT, SYS_THREAD_JOIN,
    SYS_THREAD_SUSPEND, SYS_THREAD_RESUME, SYS_THREAD_SET_PRIORITY,
    SYS_SET_FS_BASE,
    SYS_IO_RING_DESTROY, SYS_IO_RING_ENTER, SYS_IO_RING_SETUP,
    SYS_SEM_CREATE, SYS_SEM_SIGNAL, SYS_SEM_WAIT, SYS_SEM_TRY_WAIT, SYS_SEM_CLOSE,
    SYS_SEM_WAIT_TIMEOUT,
    SYS_SERVICE_REGISTER, SYS_SERVICE_CONNECT, SYS_SERVICE_ACCEPT,
    SYS_SERVICE_TRY_ACCEPT, SYS_SERVICE_ACCEPT_TIMEOUT, SYS_SERVICE_UNREGISTER,
    SYS_NS_CREATE, SYS_NS_BIND, SYS_NS_UNBIND, SYS_NS_HIDE,
    SYS_NS_ATTACH, SYS_NS_QUERY,
    SYS_CHANNEL_RECV_TIMEOUT,
    SYS_CHANNEL_SEND_CAPS, SYS_CHANNEL_RECV_CAPS,
    SYS_PIPE_READ_TIMEOUT, SYS_PIPE_WRITE_TIMEOUT,
    SYS_PIPE_PEEK, SYS_PIPE_WAIT_READABLE,
    SYS_UDP_BIND, SYS_UDP_CLOSE, SYS_UDP_RECV, SYS_UDP_SEND,
    SYS_UDP_CONNECT, SYS_UDP_LOCAL_PORT, SYS_UDP_MCAST_JOIN, SYS_UDP_MCAST_LEAVE,
    SYS_DNS_RESOLVE, SYS_DNS_REVERSE_RESOLVE,
    SYS_NET_STAT,
    SYS_ICMP_PING, SYS_ICMP_PING_WAIT,
    SYS_TCP_LIST, SYS_TCP_LISTENER_LIST, SYS_NET_IF_INFO,
    SYS_NET_IF_CONFIG,
    SYS_NET_ROUTE_ADD, SYS_NET_ROUTE_DEL, SYS_NET_ROUTE_LIST,
    SYS_NET_FW_ENABLE, SYS_NET_FW_SET_POLICY, SYS_NET_FW_ADD_RULE,
    SYS_NET_FW_DEL_RULE, SYS_NET_FW_FLUSH,
    SYS_NET_RAW_OPEN, SYS_NET_RAW_TX, SYS_NET_RAW_RX, SYS_NET_RAW_CLOSE,
    SYS_ARP_TABLE, SYS_DNS_CACHE_STATS,
    SYS_TCP_POLL_STATUS, SYS_TCP_LISTENER_READY,
    SYS_UDP_RX_READY, SYS_UDP_RX_FRONT_BYTES,
    SYS_TCP_SHUTDOWN, SYS_TCP_INFO,
    SYS_TCP_SET_NODELAY, SYS_TCP_SET_KEEPALIVE, SYS_TCP_SET_KEEPALIVE_PARAMS,
    SYS_TCP_LAST_ERROR, SYS_TCP_LOCAL_PORT,
    SYS_DRM_OPEN, SYS_DRM_CLOSE, SYS_DRM_DISPLAY_SIZE,
    SYS_DRM_GEM_CREATE, SYS_DRM_GEM_DESTROY, SYS_DRM_GEM_MMAP,
    SYS_DRM_FB_CREATE, SYS_DRM_FB_DESTROY,
    SYS_DRM_PAGE_FLIP, SYS_DRM_FLUSH_REGION,
    SYS_DRM_CONNECTOR_STATUS, SYS_DRM_MODE_GET, SYS_DRM_CRTC_INFO,
    SYS_DRM_CURSOR_SET, SYS_DRM_CURSOR_MOVE,
    SYS_DRM_ATOMIC_COMMIT,
    SYS_YIELD,
};
use super::handlers;
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
}

impl SyscallResult {
    /// Success with a single return value.
    #[must_use]
    pub const fn ok(value: i64) -> Self {
        Self { value, value2: 0 }
    }

    /// Success returning two values.
    #[must_use]
    pub const fn ok2(value: i64, value2: i64) -> Self {
        Self { value, value2 }
    }

    /// Error result.
    #[must_use]
    #[allow(clippy::cast_lossless)]
    pub const fn err(e: KernelError) -> Self {
        // `as i64` is lossless (i32 → i64) but `From` isn't const-stable.
        Self {
            value: e.code() as i64,
            value2: 0,
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
    let mut handlers: [Option<SyscallHandler>; MAX_SYSCALL_NR] =
        [None; MAX_SYSCALL_NR];

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
    handlers[SYS_SOCKETPAIR_SEND_TIMEOUT as usize] =
        Some(handlers::sys_socketpair_send_timeout);
    handlers[SYS_SOCKETPAIR_RECV_TIMEOUT as usize] =
        Some(handlers::sys_socketpair_recv_timeout);
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
    handlers[SYS_PROCESS_ID as usize] = Some(handlers::sys_process_id);
    handlers[SYS_SET_EXCEPTION_HANDLER as usize] = Some(handlers::sys_set_exception_handler);
    handlers[SYS_PROCESS_KILL as usize] = Some(handlers::sys_process_kill);
    handlers[SYS_NOTIFY_READY as usize] = Some(handlers::sys_notify_ready);
    handlers[SYS_PROCESS_IS_READY as usize] = Some(handlers::sys_process_is_ready);
    handlers[SYS_PROCESS_SPAWN_EX as usize] = Some(handlers::sys_process_spawn_ex);
    handlers[SYS_PROCESS_GET_INITIAL_FDS as usize] = Some(handlers::sys_process_get_initial_fds);
    handlers[SYS_PROCESS_GET_ARGS as usize] = Some(handlers::sys_process_get_args);
    handlers[SYS_PROCESS_PARENT_ID as usize] = Some(handlers::sys_process_parent_id);
    handlers[SYS_PROCESS_COUNT as usize] = Some(handlers::sys_process_count);

    // POSIX signal shim (522–526). SYS_SIGNAL_RETURN (524) is a
    // frame-modifying syscall handled specially in syscall_handler_inner,
    // so it has no flat-table entry.
    handlers[SYS_SIGNAL_REGISTER as usize] = Some(handlers::sys_signal_register);
    handlers[SYS_SIGNAL_SEND as usize] = Some(handlers::sys_signal_send);
    handlers[SYS_SIGNAL_MASK as usize] = Some(handlers::sys_signal_mask);
    handlers[SYS_SIGNAL_PENDING as usize] = Some(handlers::sys_signal_pending);

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

    crate::ktrace::record(
        crate::ktrace::Category::Syscall,
        crate::ktrace::event::SYSCALL_ENTER,
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
    let task_id = crate::sched::current_task_id();
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
            nr, V1_TABLE.version
        );
        SyscallResult::err(KernelError::NotSupported)
    };

    crate::ktrace::record(
        crate::ktrace::Category::Syscall,
        crate::ktrace::event::SYSCALL_EXIT,
        nr,
        result.value as u64,
    );

    // Per-process I/O byte accounting for `/proc/<pid>/io` (rchar/wchar,
    // syscr/syscw).  The Linux-ABI dispatch path accounts its own
    // read/write family separately in `linux::dispatch_linux`; this hook
    // covers the *native* read/write syscalls so native processes get
    // honest io counters instead of all-zero.  `task_id` is already
    // resolved above for the syscall filter, so this adds no extra lookup.
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
        SYS_FS_READ | SYS_FS_READ_FILE | SYS_PIPE_READ | SYS_PIPE_TRY_READ
        | SYS_PIPE_READ_TIMEOUT | SYS_CONSOLE_READ_CHAR
        | SYS_CONSOLE_TRY_READ_CHAR => Some(IoDir::Read),
        SYS_FS_WRITE | SYS_PIPE_WRITE | SYS_PIPE_TRY_WRITE
        | SYS_PIPE_WRITE_TIMEOUT | SYS_CONSOLE_WRITE => Some(IoDir::Write),
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
    test_dispatch_clock_settime()?;
    test_dispatch_clock_adjtime()?;
    test_dispatch_console_write()?;
    test_dispatch_fs_roundtrip()?;
    test_io_dir_classification()?;
    test_dispatch_mprotect_native()?;

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
        arg0, arg1, arg2, arg3: 0, arg4: 0, arg5: 0,
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
        serial_println!("[syscall]   FAIL: native mprotect len=0 returned {}, expected 0", r.value);
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

/// Verify the native read/write syscall classification feeding
/// `/proc/<pid>/io` accounting.  The byte-folding side effect itself is
/// covered by `proc::pcb::test_io_accounting`; here we pin down the
/// syscall-number → direction mapping so a misfiled number is caught.
fn test_io_dir_classification() -> KernelResult<()> {
    // Reads.
    for nr in [
        SYS_FS_READ, SYS_FS_READ_FILE, SYS_PIPE_READ, SYS_PIPE_TRY_READ,
        SYS_PIPE_READ_TIMEOUT, SYS_CONSOLE_READ_CHAR, SYS_CONSOLE_TRY_READ_CHAR,
    ] {
        if io_dir_for_syscall(nr) != Some(IoDir::Read) {
            serial_println!("[syscall]   FAIL: nr {} not classified as Read", nr);
            return Err(KernelError::InternalError);
        }
    }
    // Writes.
    for nr in [
        SYS_FS_WRITE, SYS_PIPE_WRITE, SYS_PIPE_TRY_WRITE, SYS_PIPE_WRITE_TIMEOUT,
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
        arg0: 0, arg1: 0, arg2: 0,
        arg3: 0, arg4: 0, arg5: 0,
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
        arg0: 0, arg1: 0, arg2: 0,
        arg3: 0, arg4: 0, arg5: 0,
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
            result.value, expected
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[syscall]   Dispatch SYS_TASK_ID: OK (id={})", result.value);
    Ok(())
}

fn test_dispatch_unimplemented() -> KernelResult<()> {
    let args = SyscallArgs {
        arg0: 0, arg1: 0, arg2: 0,
        arg3: 0, arg4: 0, arg5: 0,
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
        arg0: 0, arg1: 0, arg2: 0,
        arg3: 0, arg4: 0, arg5: 0,
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
        arg0: 0, arg1: 0, arg2: 0,
        arg3: 0, arg4: 0, arg5: 0,
    };
    let result = dispatch(SYS_CHANNEL_CREATE, &args);
    if result.value < 0 {
        serial_println!(
            "[syscall]   FAIL: channel_create returned {}",
            result.value
        );
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
        arg3: 0, arg4: 0, arg5: 0,
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
        arg3: 0, arg4: 0, arg5: 0,
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
        arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    dispatch(SYS_CHANNEL_CLOSE, &close0_args);

    let close1_args = SyscallArgs {
        arg0: ep1_raw,
        arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    dispatch(SYS_CHANNEL_CLOSE, &close1_args);

    serial_println!("[syscall]   Dispatch channel roundtrip: OK");
    Ok(())
}

/// Test clock_monotonic syscall returns a non-negative nanosecond value.
fn test_dispatch_clock_monotonic() -> KernelResult<()> {
    let args = SyscallArgs {
        arg0: 0, arg1: 0, arg2: 0,
        arg3: 0, arg4: 0, arg5: 0,
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

/// Test clock_realtime syscall returns a non-negative nanosecond value.
///
/// The value may be 0 if timekeeping was not initialized (no usable RTC),
/// which is still a valid (non-error) result; we only reject negative
/// (error) returns.
fn test_dispatch_clock_realtime() -> KernelResult<()> {
    let args = SyscallArgs {
        arg0: 0, arg1: 0, arg2: 0,
        arg3: 0, arg4: 0, arg5: 0,
    };
    let result = dispatch(SYS_CLOCK_REALTIME, &args);
    if result.value < 0 {
        serial_println!(
            "[syscall]   FAIL: clock_realtime returned {}",
            result.value
        );
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
        arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    let before = dispatch(SYS_CLOCK_REALTIME, &read).value;

    let set = SyscallArgs {
        #[allow(clippy::cast_sign_loss)]
        arg0: before.max(0) as u64,
        arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
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
                before, after
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
        arg0: 0, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    let before = dispatch(SYS_CLOCK_REALTIME, &read).value;

    let forward = SyscallArgs {
        arg0: STEP_NS, arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
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
            arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
        };
        let _ = dispatch(SYS_CLOCK_ADJTIME, &restore);

        if after < before {
            serial_println!(
                "[syscall]   FAIL: clock_adjtime moved time backwards ({} -> {})",
                before, after
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
        arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    let result = dispatch(SYS_CONSOLE_WRITE, &args);
    if result.value < 0 {
        serial_println!(
            "[syscall]   FAIL: console_write returned {}",
            result.value
        );
        return Err(KernelError::InternalError);
    }
    #[allow(clippy::cast_possible_wrap)]
    let expected_len = msg.len() as i64;
    if result.value != expected_len {
        serial_println!(
            "[syscall]   FAIL: console_write returned {}, expected {}",
            result.value, expected_len
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
        arg4: 0, arg5: 0,
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
        arg4: 0, arg5: 0,
    };
    let read_result = dispatch(SYS_FS_READ_FILE, &read_args);
    #[allow(clippy::cast_possible_wrap)]
    let expected_len = test_data.len() as i64;
    if read_result.value != expected_len {
        serial_println!(
            "[syscall]   FAIL: read_file returned {}, expected {}",
            read_result.value, expected_len
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
        arg3: 0, arg4: 0, arg5: 0,
    };
    let stat_result = dispatch(SYS_FS_STAT, &stat_args);
    if stat_result.value != 0 {
        serial_println!(
            "[syscall]   FAIL: stat returned {}",
            stat_result.value
        );
        return Err(KernelError::InternalError);
    }
    // Verify size field (bytes 0-7, u64 LE).
    let stat_size = u64::from_le_bytes([
        stat_buf[0], stat_buf[1], stat_buf[2], stat_buf[3],
        stat_buf[4], stat_buf[5], stat_buf[6], stat_buf[7],
    ]);
    if stat_size != test_data.len() as u64 {
        serial_println!(
            "[syscall]   FAIL: stat size {} != expected {}",
            stat_size, test_data.len()
        );
        return Err(KernelError::InternalError);
    }
    // Verify type field (byte 8): 0=file.
    if stat_buf[8] != 0 {
        serial_println!(
            "[syscall]   FAIL: stat type {} != 0 (file)",
            stat_buf[8]
        );
        return Err(KernelError::InternalError);
    }

    // 4. Delete the test file.
    let delete_args = SyscallArgs {
        arg0: test_path.as_ptr() as u64,
        arg1: test_path.len() as u64,
        arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    let delete_result = dispatch(SYS_FS_DELETE, &delete_args);
    if delete_result.value != 0 {
        serial_println!(
            "[syscall]   FAIL: delete returned {}",
            delete_result.value
        );
        return Err(KernelError::InternalError);
    }

    // 5. Test mkdir + rmdir.
    let dir_path = b"/syscall_test_dir";
    let mkdir_args = SyscallArgs {
        arg0: dir_path.as_ptr() as u64,
        arg1: dir_path.len() as u64,
        arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    let mkdir_result = dispatch(SYS_FS_MKDIR, &mkdir_args);
    if mkdir_result.value != 0 {
        serial_println!(
            "[syscall]   FAIL: mkdir returned {}",
            mkdir_result.value
        );
        return Err(KernelError::InternalError);
    }

    // Stat the directory.
    let stat_dir_args = SyscallArgs {
        arg0: dir_path.as_ptr() as u64,
        arg1: dir_path.len() as u64,
        arg2: stat_buf.as_mut_ptr() as u64,
        arg3: 0, arg4: 0, arg5: 0,
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
        serial_println!(
            "[syscall]   FAIL: stat dir type {} != 1",
            stat_buf[8]
        );
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
        arg4: 0, arg5: 0,
    };
    let list_result = dispatch(SYS_FS_LIST_DIR, &list_args);
    if list_result.value < 0 {
        serial_println!(
            "[syscall]   FAIL: list_dir returned {}",
            list_result.value
        );
        return Err(KernelError::InternalError);
    }

    // Remove the test directory.
    let rmdir_args = SyscallArgs {
        arg0: dir_path.as_ptr() as u64,
        arg1: dir_path.len() as u64,
        arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    let rmdir_result = dispatch(SYS_FS_RMDIR, &rmdir_args);
    if rmdir_result.value != 0 {
        serial_println!(
            "[syscall]   FAIL: rmdir returned {}",
            rmdir_result.value
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[syscall]   Dispatch FS roundtrip: OK (write/read/stat/delete/mkdir/listdir/rmdir)");
    Ok(())
}
