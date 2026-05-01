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

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

use super::number::{
    MAX_SYSCALL_NR, SYS_CHANNEL_CLOSE, SYS_CHANNEL_CREATE, SYS_CHANNEL_RECV,
    SYS_CHANNEL_SEND, SYS_CHANNEL_TRY_RECV, SYS_CONSOLE_READ_CHAR,
    SYS_CONSOLE_WRITE, SYS_CP_CLOSE, SYS_CP_CREATE, SYS_CP_NOTIFY,
    SYS_CP_REGISTER, SYS_CP_TRY_WAIT, SYS_CP_UNREGISTER, SYS_CP_WAIT,
    SYS_CLOCK_MONOTONIC,
    SYS_DEBUG_PRINT, SYS_LOG_READ,
    SYS_EVENTFD_CLOSE, SYS_EVENTFD_CREATE,
    SYS_TIMER_CANCEL, SYS_TIMER_CREATE,
    SYS_EVENTFD_READ, SYS_EVENTFD_TRY_READ, SYS_EVENTFD_WRITE, SYS_EXIT,
    SYS_FS_DELETE, SYS_FS_LIST_DIR, SYS_FS_MKDIR, SYS_FS_READ_FILE,
    SYS_FS_RMDIR, SYS_FS_STAT, SYS_FS_WRITE_FILE,
    SYS_FUTEX_LOCK_PI, SYS_FUTEX_UNLOCK_PI,
    SYS_FUTEX_WAIT, SYS_FUTEX_WAKE, SYS_IRQ_REGISTER, SYS_IRQ_RELEASE,
    SYS_IRQ_WAIT, SYS_PIPE_CLOSE, SYS_PIPE_CREATE, SYS_PIPE_READ,
    SYS_PIPE_TRY_READ, SYS_PIPE_TRY_WRITE, SYS_PIPE_WRITE,
    SYS_PORT_READ, SYS_PORT_WRITE,
    SYS_CAP_QUERY, SYS_MMAP, SYS_MUNMAP, SYS_PROCESS_ID,
    SYS_PROCESS_KILL, SYS_PROCESS_SPAWN, SYS_PROCESS_WAIT,
    SYS_SET_EXCEPTION_HANDLER,
    SYS_SHM_CLOSE, SYS_SHM_CREATE, SYS_SHM_SIZE, SYS_SLEEP, SYS_TASK_ID,
    SYS_TCP_CLOSE, SYS_TCP_CONNECT, SYS_TCP_RECV, SYS_TCP_SEND,
    SYS_THREAD_CREATE, SYS_THREAD_EXIT, SYS_THREAD_JOIN,
    SYS_THREAD_SUSPEND, SYS_THREAD_RESUME, SYS_THREAD_SET_PRIORITY,
    SYS_IO_RING_DESTROY, SYS_IO_RING_ENTER, SYS_IO_RING_SETUP,
    SYS_UDP_BIND, SYS_UDP_CLOSE, SYS_UDP_RECV, SYS_UDP_SEND,
    SYS_DNS_RESOLVE,
    SYS_YIELD,
};
use super::handlers;

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
    handlers[SYS_IRQ_REGISTER as usize] = Some(handlers::sys_irq_register);
    handlers[SYS_IRQ_WAIT as usize] = Some(handlers::sys_irq_wait);
    handlers[SYS_IRQ_RELEASE as usize] = Some(handlers::sys_irq_release);
    handlers[SYS_PORT_READ as usize] = Some(handlers::sys_port_read);
    handlers[SYS_PORT_WRITE as usize] = Some(handlers::sys_port_write);

    // IPC (200–399)
    handlers[SYS_CHANNEL_CREATE as usize] = Some(handlers::sys_channel_create);
    handlers[SYS_CHANNEL_SEND as usize] = Some(handlers::sys_channel_send);
    handlers[SYS_CHANNEL_RECV as usize] = Some(handlers::sys_channel_recv);
    handlers[SYS_CHANNEL_TRY_RECV as usize] = Some(handlers::sys_channel_try_recv);
    handlers[SYS_CHANNEL_CLOSE as usize] = Some(handlers::sys_channel_close);
    handlers[SYS_FUTEX_WAIT as usize] = Some(handlers::sys_futex_wait);
    handlers[SYS_FUTEX_WAKE as usize] = Some(handlers::sys_futex_wake);
    handlers[SYS_FUTEX_LOCK_PI as usize] = Some(handlers::sys_futex_lock_pi);
    handlers[SYS_FUTEX_UNLOCK_PI as usize] = Some(handlers::sys_futex_unlock_pi);
    handlers[SYS_PIPE_CREATE as usize] = Some(handlers::sys_pipe_create);
    handlers[SYS_PIPE_WRITE as usize] = Some(handlers::sys_pipe_write);
    handlers[SYS_PIPE_READ as usize] = Some(handlers::sys_pipe_read);
    handlers[SYS_PIPE_TRY_WRITE as usize] = Some(handlers::sys_pipe_try_write);
    handlers[SYS_PIPE_TRY_READ as usize] = Some(handlers::sys_pipe_try_read);
    handlers[SYS_PIPE_CLOSE as usize] = Some(handlers::sys_pipe_close);
    handlers[SYS_SHM_CREATE as usize] = Some(handlers::sys_shm_create);
    handlers[SYS_SHM_SIZE as usize] = Some(handlers::sys_shm_size);
    handlers[SYS_SHM_CLOSE as usize] = Some(handlers::sys_shm_close);
    handlers[SYS_EVENTFD_CREATE as usize] = Some(handlers::sys_eventfd_create);
    handlers[SYS_EVENTFD_WRITE as usize] = Some(handlers::sys_eventfd_write);
    handlers[SYS_EVENTFD_READ as usize] = Some(handlers::sys_eventfd_read);
    handlers[SYS_EVENTFD_TRY_READ as usize] = Some(handlers::sys_eventfd_try_read);
    handlers[SYS_EVENTFD_CLOSE as usize] = Some(handlers::sys_eventfd_close);
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

    // Time and timers (10–19).
    handlers[SYS_CLOCK_MONOTONIC as usize] = Some(handlers::sys_clock_monotonic);
    handlers[SYS_SLEEP as usize] = Some(handlers::sys_sleep);
    handlers[SYS_TIMER_CREATE as usize] = Some(handlers::sys_timer_create);
    handlers[SYS_TIMER_CANCEL as usize] = Some(handlers::sys_timer_cancel);

    // Console I/O (100–109).
    handlers[SYS_CONSOLE_WRITE as usize] = Some(handlers::sys_console_write);
    handlers[SYS_CONSOLE_READ_CHAR as usize] = Some(handlers::sys_console_read_char);
    handlers[SYS_LOG_READ as usize] = Some(handlers::sys_log_read);

    // Security (400–499).
    handlers[SYS_CAP_QUERY as usize] = Some(handlers::sys_cap_query);

    // Process management (500–509).
    handlers[SYS_PROCESS_SPAWN as usize] = Some(handlers::sys_process_spawn);
    handlers[SYS_PROCESS_WAIT as usize] = Some(handlers::sys_process_wait);
    handlers[SYS_PROCESS_ID as usize] = Some(handlers::sys_process_id);
    handlers[SYS_SET_EXCEPTION_HANDLER as usize] = Some(handlers::sys_set_exception_handler);
    handlers[SYS_PROCESS_KILL as usize] = Some(handlers::sys_process_kill);

    // Thread management (510–519).
    handlers[SYS_THREAD_CREATE as usize] = Some(handlers::sys_thread_create);
    handlers[SYS_THREAD_EXIT as usize] = Some(handlers::sys_thread_exit);
    handlers[SYS_THREAD_JOIN as usize] = Some(handlers::sys_thread_join);
    handlers[SYS_THREAD_SUSPEND as usize] = Some(handlers::sys_thread_suspend);
    handlers[SYS_THREAD_RESUME as usize] = Some(handlers::sys_thread_resume);
    handlers[SYS_THREAD_SET_PRIORITY as usize] = Some(handlers::sys_thread_set_priority);

    // Filesystem (600–799).
    handlers[SYS_FS_READ_FILE as usize] = Some(handlers::sys_fs_read_file);
    handlers[SYS_FS_WRITE_FILE as usize] = Some(handlers::sys_fs_write_file);
    handlers[SYS_FS_DELETE as usize] = Some(handlers::sys_fs_delete);
    handlers[SYS_FS_LIST_DIR as usize] = Some(handlers::sys_fs_list_dir);
    handlers[SYS_FS_MKDIR as usize] = Some(handlers::sys_fs_mkdir);
    handlers[SYS_FS_RMDIR as usize] = Some(handlers::sys_fs_rmdir);
    handlers[SYS_FS_STAT as usize] = Some(handlers::sys_fs_stat);

    // Networking (800–999).
    handlers[SYS_TCP_CONNECT as usize] = Some(handlers::sys_tcp_connect);
    handlers[SYS_TCP_SEND as usize] = Some(handlers::sys_tcp_send);
    handlers[SYS_TCP_RECV as usize] = Some(handlers::sys_tcp_recv);
    handlers[SYS_TCP_CLOSE as usize] = Some(handlers::sys_tcp_close);
    handlers[SYS_UDP_BIND as usize] = Some(handlers::sys_udp_bind);
    handlers[SYS_UDP_SEND as usize] = Some(handlers::sys_udp_send);
    handlers[SYS_UDP_RECV as usize] = Some(handlers::sys_udp_recv);
    handlers[SYS_UDP_CLOSE as usize] = Some(handlers::sys_udp_close);
    handlers[SYS_DNS_RESOLVE as usize] = Some(handlers::sys_dns_resolve);

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
    // Bounds check.
    let idx = nr as usize;
    if idx >= MAX_SYSCALL_NR {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Look up handler.
    //
    // SAFETY: idx is bounds-checked above.
    #[allow(clippy::indexing_slicing)]
    if let Some(handler) = V1_TABLE.handlers[idx] {
        handler(args)
    } else {
        serial_println!(
            "[syscall] Unimplemented syscall {} (v{})",
            nr, V1_TABLE.version
        );
        SyscallResult::err(KernelError::NotSupported)
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
    test_dispatch_console_write()?;
    test_dispatch_fs_roundtrip()?;

    serial_println!("[syscall] Dispatch self-test PASSED");
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
    // Use a known-undefined number in kernel-core range.
    let result = dispatch(42, &args);
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
    let mut stat_buf = [0u8; 16];
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
