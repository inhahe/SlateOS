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
    SYS_CHANNEL_SEND, SYS_CHANNEL_TRY_RECV, SYS_CP_CLOSE, SYS_CP_CREATE,
    SYS_CP_NOTIFY, SYS_CP_REGISTER, SYS_CP_TRY_WAIT, SYS_CP_UNREGISTER,
    SYS_CP_WAIT, SYS_DEBUG_PRINT, SYS_EVENTFD_CLOSE, SYS_EVENTFD_CREATE,
    SYS_EVENTFD_READ, SYS_EVENTFD_TRY_READ, SYS_EVENTFD_WRITE, SYS_EXIT,
    SYS_FUTEX_WAIT, SYS_FUTEX_WAKE, SYS_PIPE_CLOSE, SYS_PIPE_CREATE,
    SYS_PIPE_READ, SYS_PIPE_TRY_READ, SYS_PIPE_TRY_WRITE, SYS_PIPE_WRITE,
    SYS_PROCESS_ID, SYS_PROCESS_SPAWN, SYS_PROCESS_WAIT,
    SYS_SHM_CLOSE, SYS_SHM_CREATE, SYS_SHM_SIZE, SYS_SLEEP, SYS_TASK_ID,
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

    // IPC (200–399)
    handlers[SYS_CHANNEL_CREATE as usize] = Some(handlers::sys_channel_create);
    handlers[SYS_CHANNEL_SEND as usize] = Some(handlers::sys_channel_send);
    handlers[SYS_CHANNEL_RECV as usize] = Some(handlers::sys_channel_recv);
    handlers[SYS_CHANNEL_TRY_RECV as usize] = Some(handlers::sys_channel_try_recv);
    handlers[SYS_CHANNEL_CLOSE as usize] = Some(handlers::sys_channel_close);
    handlers[SYS_FUTEX_WAIT as usize] = Some(handlers::sys_futex_wait);
    handlers[SYS_FUTEX_WAKE as usize] = Some(handlers::sys_futex_wake);
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

    // Process management (500–599).
    handlers[SYS_PROCESS_SPAWN as usize] = Some(handlers::sys_process_spawn);
    handlers[SYS_PROCESS_WAIT as usize] = Some(handlers::sys_process_wait);
    handlers[SYS_PROCESS_ID as usize] = Some(handlers::sys_process_id);

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
    // SYS_SLEEP is defined but not yet implemented.
    let result = dispatch(SYS_SLEEP, &args);
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
