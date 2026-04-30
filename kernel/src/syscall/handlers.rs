//! Syscall handler implementations.
//!
//! Each handler is a function that takes [`SyscallArgs`] and returns a
//! [`SyscallResult`].  Handlers are referenced from the dispatch table
//! in [`super::dispatch`].
//!
//! ## Conventions
//!
//! - Handlers are named `sys_<operation>`.
//! - On success, `SyscallResult::value` is the return value (>= 0).
//! - On error, `SyscallResult::value` is the negative error code
//!   from [`KernelError`].
//! - Pointer arguments from userspace must be validated before
//!   dereferencing.  For now (kernel-mode only), we trust pointers
//!   but add TODO markers for userspace validation.

// Syscall args are u64 (register-width).  On our x86_64 target,
// usize is 64 bits, so u64→usize casts cannot truncate.
#![allow(clippy::cast_possible_truncation)]

use crate::error::KernelError;
use crate::ipc::channel::{self, ChannelHandle, Message};
use crate::ipc::completion::{self, CpHandle, WaitSource};
use crate::ipc::eventfd::{self, EventFdHandle};
use crate::ipc::futex;
use crate::ipc::pipe::{self, PipeHandle};
use crate::ipc::shm::{self, ShmHandle};
use crate::sched;
use crate::serial_println;

use super::dispatch::{SyscallArgs, SyscallResult};

// ---------------------------------------------------------------------------
// Kernel-core handlers (0–199)
// ---------------------------------------------------------------------------

/// `SYS_YIELD` — yield the current task's time slice.
pub fn sys_yield(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    sched::yield_now();
    SyscallResult::ok(0)
}

/// `SYS_EXIT` — exit the current task.
///
/// Notifies the thread/process system before terminating.  If this
/// was the last thread in a process, the process becomes a zombie.
pub fn sys_exit(args: &SyscallArgs) -> SyscallResult {
    // TODO: Store exit code (args.arg0) for parent to retrieve.
    let _ = args;

    // Notify the thread system so the owning process can transition
    // to Zombie when its last thread exits.  For bare kernel tasks
    // (not owned by any process), this is a harmless no-op.
    let task_id = sched::current_task_id();
    crate::proc::thread::on_thread_exit(task_id);

    sched::task_exit();
    // Unreachable — task_exit never returns.
    SyscallResult::ok(0)
}

/// `SYS_TASK_ID` — get the current task's ID.
pub fn sys_task_id(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    #[allow(clippy::cast_possible_wrap)]
    let id = sched::current_task_id() as i64;
    SyscallResult::ok(id)
}

/// `SYS_DEBUG_PRINT` — print a byte string to serial (debug only).
///
/// # Safety contract
///
/// `arg0` must be a valid pointer to `arg1` bytes of readable memory.
/// For now (kernel-mode testing), we trust the pointer.  When
/// userspace is implemented, this must validate the pointer against
/// the caller's address space.
pub fn sys_debug_print(args: &SyscallArgs) -> SyscallResult {
    let ptr = args.arg0 as *const u8;
    let len = args.arg1 as usize;

    // TODO: Validate pointer is in caller's address space.
    // For now, only kernel-mode callers use this.
    if ptr.is_null() || len == 0 {
        return SyscallResult::ok(0);
    }

    // Cap length to prevent excessive output.
    let safe_len = len.min(1024);

    // SAFETY: Caller guarantees ptr is valid for safe_len bytes.
    // In kernel mode (current stage), all memory is accessible.
    let bytes = unsafe { core::slice::from_raw_parts(ptr, safe_len) };

    // Print as UTF-8 if valid, otherwise as hex.
    if let Ok(s) = core::str::from_utf8(bytes) {
        serial_println!("[debug] {}", s);
    } else {
        serial_println!("[debug] <{} non-UTF8 bytes>", safe_len);
    }

    #[allow(clippy::cast_possible_wrap)]
    let written = safe_len as i64;
    SyscallResult::ok(written)
}

// ---------------------------------------------------------------------------
// IPC handlers (200–399)
// ---------------------------------------------------------------------------

/// `SYS_CHANNEL_CREATE` — create a new IPC channel pair.
///
/// Returns both handles: `value` = ep0, `value2` = ep1.
pub fn sys_channel_create(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    let (ep0, ep1) = channel::create();

    // Pack handles into the two return registers.
    #[allow(clippy::cast_possible_wrap)]
    let r0 = ep0.raw() as i64;
    #[allow(clippy::cast_possible_wrap)]
    let r1 = ep1.raw() as i64;
    SyscallResult::ok2(r0, r1)
}

/// `SYS_CHANNEL_SEND` — send a message on a channel.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to message data.
/// `arg2`: length of message data.
pub fn sys_channel_send(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    let ptr = args.arg1 as *const u8;
    let len = args.arg2 as usize;

    // TODO: Validate pointer is in caller's address space.
    if ptr.is_null() && len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // SAFETY: Caller guarantees ptr is valid for len bytes.
    let data = if len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };

    let msg = match Message::from_bytes(data) {
        Ok(m) => m,
        Err(e) => return SyscallResult::err(e),
    };

    match channel::send(handle, msg) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CHANNEL_RECV` — blocking receive on a channel.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
///
/// Returns: message length on success.
pub fn sys_channel_recv(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;

    // TODO: Validate pointer is in caller's address space.
    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    match channel::recv(handle) {
        Ok(msg) => {
            let data = msg.data();
            let copy_len = data.len().min(buf_cap);

            if copy_len > 0 {
                // SAFETY: Caller guarantees buf_ptr is valid for buf_cap bytes.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr(),
                        buf_ptr,
                        copy_len,
                    );
                }
            }

            #[allow(clippy::cast_possible_wrap)]
            let len = data.len() as i64;
            SyscallResult::ok(len)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CHANNEL_TRY_RECV` — non-blocking receive on a channel.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
///
/// Returns: message length, 0 if empty, negative error code on failure.
pub fn sys_channel_try_recv(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;

    // TODO: Validate pointer is in caller's address space.
    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    match channel::try_recv(handle) {
        Ok(Some(msg)) => {
            let data = msg.data();
            let copy_len = data.len().min(buf_cap);

            if copy_len > 0 {
                // SAFETY: Caller guarantees buf_ptr is valid for buf_cap bytes.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr(),
                        buf_ptr,
                        copy_len,
                    );
                }
            }

            #[allow(clippy::cast_possible_wrap)]
            let len = data.len() as i64;
            SyscallResult::ok(len)
        }
        Ok(None) => {
            // No message available — return 0 (not an error).
            SyscallResult::ok(0)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CHANNEL_CLOSE` — close a channel endpoint.
///
/// `arg0`: channel handle.
pub fn sys_channel_close(args: &SyscallArgs) -> SyscallResult {
    let handle = ChannelHandle::from_raw(args.arg0);
    channel::close(handle);
    SyscallResult::ok(0)
}

/// `SYS_FUTEX_WAIT` — block if `*addr == expected`.
///
/// `arg0`: pointer to 32-bit futex word (4-byte aligned).
/// `arg1`: expected value.
///
/// Returns: 1 if blocked and woken, 0 if value didn't match.
pub fn sys_futex_wait(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;
    let expected = args.arg1 as u32;

    // TODO: Validate pointer is in caller's address space.
    match futex::futex_wait(addr, expected) {
        Ok(true) => SyscallResult::ok(1),
        Ok(false) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_FUTEX_WAKE` — wake up to `max_wake` waiters on a futex.
///
/// `arg0`: pointer to futex word.
/// `arg1`: maximum number of tasks to wake.
///
/// Returns: number of tasks actually woken.
pub fn sys_futex_wake(args: &SyscallArgs) -> SyscallResult {
    let addr = args.arg0;
    let max_wake = args.arg1 as u32;

    let woken = futex::futex_wake(addr, max_wake);
    SyscallResult::ok(i64::from(woken))
}

// ---------------------------------------------------------------------------
// Pipe handlers (220–229)
// ---------------------------------------------------------------------------

/// `SYS_PIPE_CREATE` — create a one-way pipe.
///
/// Returns both handles: `value` = read end, `value2` = write end.
pub fn sys_pipe_create(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    let (read_handle, write_handle) = pipe::create();

    #[allow(clippy::cast_possible_wrap)]
    let r0 = read_handle.raw() as i64;
    #[allow(clippy::cast_possible_wrap)]
    let r1 = write_handle.raw() as i64;
    SyscallResult::ok2(r0, r1)
}

/// `SYS_PIPE_WRITE` — write bytes to a pipe (blocking).
///
/// `arg0`: write-end pipe handle.
/// `arg1`: pointer to data buffer.
/// `arg2`: number of bytes to write.
///
/// Returns: number of bytes written.
pub fn sys_pipe_write(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let ptr = args.arg1 as *const u8;
    let len = args.arg2 as usize;

    // TODO: Validate pointer is in caller's address space.
    if ptr.is_null() && len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let data = if len == 0 {
        &[]
    } else {
        // SAFETY: Caller guarantees ptr is valid for len bytes.
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };

    match pipe::write(handle, data) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let written = n as i64;
            SyscallResult::ok(written)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PIPE_READ` — read bytes from a pipe (blocking).
///
/// `arg0`: read-end pipe handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
///
/// Returns: number of bytes read (0 = EOF).
pub fn sys_pipe_read(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;

    // TODO: Validate pointer is in caller's address space.
    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let buf = if buf_cap == 0 {
        &mut []
    } else {
        // SAFETY: Caller guarantees buf_ptr is valid for buf_cap bytes.
        unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_cap) }
    };

    match pipe::read(handle, buf) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let read_bytes = n as i64;
            SyscallResult::ok(read_bytes)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PIPE_TRY_WRITE` — non-blocking write to a pipe.
///
/// Same as `SYS_PIPE_WRITE` but returns `WouldBlock` if buffer is full.
pub fn sys_pipe_try_write(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let ptr = args.arg1 as *const u8;
    let len = args.arg2 as usize;

    if ptr.is_null() && len > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let data = if len == 0 {
        &[]
    } else {
        // SAFETY: Caller guarantees ptr is valid for len bytes.
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };

    match pipe::try_write(handle, data) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let written = n as i64;
            SyscallResult::ok(written)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PIPE_TRY_READ` — non-blocking read from a pipe.
///
/// Same as `SYS_PIPE_READ` but returns `WouldBlock` if empty.
pub fn sys_pipe_try_read(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut u8;
    let buf_cap = args.arg2 as usize;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    let buf = if buf_cap == 0 {
        &mut []
    } else {
        // SAFETY: Caller guarantees buf_ptr is valid for buf_cap bytes.
        unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_cap) }
    };

    match pipe::try_read(handle, buf) {
        Ok(n) => {
            #[allow(clippy::cast_possible_wrap)]
            let read_bytes = n as i64;
            SyscallResult::ok(read_bytes)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PIPE_CLOSE` — close a pipe handle.
///
/// `arg0`: pipe handle (either end).
pub fn sys_pipe_close(args: &SyscallArgs) -> SyscallResult {
    let handle = PipeHandle::from_raw(args.arg0);
    pipe::close(handle);
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Shared memory handlers (230–239)
// ---------------------------------------------------------------------------

/// `SYS_SHM_CREATE` — create a shared memory region.
///
/// `arg0`: requested size in bytes.
///
/// Returns: shared memory handle.
pub fn sys_shm_create(args: &SyscallArgs) -> SyscallResult {
    let size = args.arg0 as usize;

    match shm::create(size) {
        Ok(handle) => {
            #[allow(clippy::cast_possible_wrap)]
            let h = handle.raw() as i64;
            SyscallResult::ok(h)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SHM_SIZE` — query the size of a shared memory region.
///
/// `arg0`: shared memory handle.
///
/// Returns: size in bytes.
pub fn sys_shm_size(args: &SyscallArgs) -> SyscallResult {
    let handle = ShmHandle::from_raw(args.arg0);

    match shm::size(handle) {
        Ok(sz) => {
            #[allow(clippy::cast_possible_wrap)]
            let s = sz as i64;
            SyscallResult::ok(s)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_SHM_CLOSE` — close a shared memory handle.
///
/// `arg0`: shared memory handle.
pub fn sys_shm_close(args: &SyscallArgs) -> SyscallResult {
    let handle = ShmHandle::from_raw(args.arg0);
    shm::close(handle);
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Eventfd handlers (240–249)
// ---------------------------------------------------------------------------

/// `SYS_EVENTFD_CREATE` — create a new eventfd counter.
///
/// `arg0`: initial counter value.
///
/// Returns: eventfd handle.
pub fn sys_eventfd_create(args: &SyscallArgs) -> SyscallResult {
    let initial = args.arg0;
    let handle = eventfd::create(initial);

    #[allow(clippy::cast_possible_wrap)]
    let h = handle.raw() as i64;
    SyscallResult::ok(h)
}

/// `SYS_EVENTFD_WRITE` — signal an eventfd (add value to counter).
///
/// `arg0`: eventfd handle.
/// `arg1`: value to add.
///
/// Returns: 0 on success.
pub fn sys_eventfd_write(args: &SyscallArgs) -> SyscallResult {
    let handle = EventFdHandle::from_raw(args.arg0);
    let value = args.arg1;

    match eventfd::write(handle, value) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_EVENTFD_READ` — consume the eventfd counter (blocking).
///
/// `arg0`: eventfd handle.
///
/// Returns: counter value (> 0).
pub fn sys_eventfd_read(args: &SyscallArgs) -> SyscallResult {
    let handle = EventFdHandle::from_raw(args.arg0);

    match eventfd::read(handle) {
        Ok(val) => {
            #[allow(clippy::cast_possible_wrap)]
            let v = val as i64;
            SyscallResult::ok(v)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_EVENTFD_TRY_READ` — non-blocking read on an eventfd.
///
/// `arg0`: eventfd handle.
///
/// Returns: counter value, or `WouldBlock` if counter is 0.
pub fn sys_eventfd_try_read(args: &SyscallArgs) -> SyscallResult {
    let handle = EventFdHandle::from_raw(args.arg0);

    match eventfd::try_read(handle) {
        Ok(val) => {
            #[allow(clippy::cast_possible_wrap)]
            let v = val as i64;
            SyscallResult::ok(v)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_EVENTFD_CLOSE` — close an eventfd handle.
///
/// `arg0`: eventfd handle.
pub fn sys_eventfd_close(args: &SyscallArgs) -> SyscallResult {
    let handle = EventFdHandle::from_raw(args.arg0);
    eventfd::close(handle);
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Completion port handlers (250–259)
// ---------------------------------------------------------------------------

/// Decode a source type + handle from syscall args into a `WaitSource`.
///
/// Source types: 0=channel, 1=`pipe_read`, 2=`pipe_write`, 3=eventfd,
/// 4=`process_exit`.
fn decode_wait_source(source_type: u64, handle: u64) -> Option<WaitSource> {
    match source_type {
        0 => Some(WaitSource::Channel(handle)),
        1 => Some(WaitSource::PipeRead(handle)),
        2 => Some(WaitSource::PipeWrite(handle)),
        3 => Some(WaitSource::EventFd(handle)),
        4 => Some(WaitSource::ProcessExit(handle)),
        _ => None,
    }
}

/// `SYS_CP_CREATE` — create a completion port.
///
/// Returns: completion port handle.
pub fn sys_cp_create(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    let handle = completion::create();

    #[allow(clippy::cast_possible_wrap)]
    let h = handle.raw() as i64;
    SyscallResult::ok(h)
}

/// `SYS_CP_REGISTER` — register a source with a completion port.
///
/// `arg0`: CP handle.
/// `arg1`: source type (0-3).
/// `arg2`: source handle.
/// `arg3`: `user_data`.
pub fn sys_cp_register(args: &SyscallArgs) -> SyscallResult {
    let cp = CpHandle::from_raw(args.arg0);

    let Some(source) = decode_wait_source(args.arg1, args.arg2) else {
        return SyscallResult::err(KernelError::InvalidArgument);
    };

    match completion::register(cp, source, args.arg3) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CP_UNREGISTER` — unregister a source from a completion port.
///
/// `arg0`: CP handle.
/// `arg1`: source type.
/// `arg2`: source handle.
pub fn sys_cp_unregister(args: &SyscallArgs) -> SyscallResult {
    let cp = CpHandle::from_raw(args.arg0);

    let Some(source) = decode_wait_source(args.arg1, args.arg2) else {
        return SyscallResult::err(KernelError::InvalidArgument);
    };

    match completion::unregister(cp, source) {
        Ok(()) => SyscallResult::ok(0),
        Err(e) => SyscallResult::err(e),
    }
}

/// Raw event structure for the syscall boundary.
///
/// Matches the layout expected by userspace.  Each event is 24 bytes:
/// `source_type` (u64) + `source_handle` (u64) + `user_data` (u64).
#[repr(C)]
struct CpEventRaw {
    source_type: u64,
    source_handle: u64,
    user_data: u64,
}

/// Encode a `CompletionEvent` into the raw format.
fn encode_event(event: &completion::CompletionEvent) -> CpEventRaw {
    let (source_type, source_handle) = match event.source {
        WaitSource::Channel(h) => (0u64, h),
        WaitSource::PipeRead(h) => (1, h),
        WaitSource::PipeWrite(h) => (2, h),
        WaitSource::EventFd(h) => (3, h),
        WaitSource::ProcessExit(h) => (4, h),
    };
    CpEventRaw {
        source_type,
        source_handle,
        user_data: event.user_data,
    }
}

/// Write events to the userspace buffer and return the count.
///
/// # Safety
///
/// `buf_ptr` must be valid for `buf_cap` `CpEventRaw` entries.
unsafe fn write_events_to_buffer(
    events: &[completion::CompletionEvent],
    buf_ptr: *mut CpEventRaw,
    buf_cap: usize,
) -> usize {
    let count = events.len().min(buf_cap);
    for (i, event) in events.iter().take(count).enumerate() {
        let raw = encode_event(event);
        // SAFETY: buf_ptr is valid for buf_cap entries, i < count <= buf_cap.
        unsafe {
            buf_ptr.add(i).write(raw);
        }
    }
    count
}

/// `SYS_CP_WAIT` — blocking wait for events.
///
/// `arg0`: CP handle.
/// `arg1`: pointer to event buffer.
/// `arg2`: buffer capacity (max events).
pub fn sys_cp_wait(args: &SyscallArgs) -> SyscallResult {
    let cp = CpHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut CpEventRaw;
    let buf_cap = args.arg2 as usize;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    match completion::wait(cp) {
        Ok(events) => {
            // SAFETY: Caller guarantees buf_ptr is valid for buf_cap entries.
            let count = unsafe { write_events_to_buffer(&events, buf_ptr, buf_cap) };
            #[allow(clippy::cast_possible_wrap)]
            let n = count as i64;
            SyscallResult::ok(n)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CP_TRY_WAIT` — non-blocking poll for events.
///
/// Same arguments as `SYS_CP_WAIT`.
pub fn sys_cp_try_wait(args: &SyscallArgs) -> SyscallResult {
    let cp = CpHandle::from_raw(args.arg0);
    let buf_ptr = args.arg1 as *mut CpEventRaw;
    let buf_cap = args.arg2 as usize;

    if buf_ptr.is_null() && buf_cap > 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    match completion::try_wait(cp) {
        Ok(events) => {
            // SAFETY: Caller guarantees buf_ptr is valid for buf_cap entries.
            let count = unsafe { write_events_to_buffer(&events, buf_ptr, buf_cap) };
            #[allow(clippy::cast_possible_wrap)]
            let n = count as i64;
            SyscallResult::ok(n)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_CP_CLOSE` — close a completion port.
///
/// `arg0`: CP handle.
pub fn sys_cp_close(args: &SyscallArgs) -> SyscallResult {
    let cp = CpHandle::from_raw(args.arg0);
    completion::close(cp);
    SyscallResult::ok(0)
}

/// `SYS_CP_NOTIFY` — manually post a notification to a completion port.
///
/// `arg0`: CP handle.
/// `arg1`: source type.
/// `arg2`: source handle.
pub fn sys_cp_notify(args: &SyscallArgs) -> SyscallResult {
    let cp = CpHandle::from_raw(args.arg0);

    let Some(source) = decode_wait_source(args.arg1, args.arg2) else {
        return SyscallResult::err(KernelError::InvalidArgument);
    };

    completion::notify(cp, source);
    SyscallResult::ok(0)
}

// ---------------------------------------------------------------------------
// Process syscalls (500–599)
// ---------------------------------------------------------------------------

/// `SYS_PROCESS_SPAWN` — spawn a new process from an ELF binary.
///
/// `arg0`: pointer to ELF data in memory.
/// `arg1`: ELF data length.
/// `arg2`: pointer to name string (UTF-8).
/// `arg3`: name length.
///
/// Returns: process ID on success, negative error on failure.
///
/// # Note
///
/// In the final implementation, `arg0`/`arg1` will be a path in the
/// filesystem, not raw ELF data.  The kernel will open the file, read
/// the ELF, and load it.  For now (no filesystem), we accept raw bytes.
pub fn sys_process_spawn(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::spawn::{SpawnOptions, spawn_process};

    let elf_ptr = args.arg0 as usize;
    let elf_len = args.arg1 as usize;
    let name_ptr = args.arg2 as usize;
    let name_len = args.arg3 as usize;

    // Validate pointers.
    // TODO: proper userspace pointer validation when we have ring 3.
    if elf_len == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Read the ELF data.
    // SAFETY: In kernel mode, we trust the pointer.  Userspace validation
    // will be added when ring 3 is implemented.
    let elf_data = unsafe {
        core::slice::from_raw_parts(elf_ptr as *const u8, elf_len)
    };

    // Read the name.
    let name = if name_len > 0 && name_ptr != 0 {
        let name_bytes = unsafe {
            core::slice::from_raw_parts(name_ptr as *const u8, name_len)
        };
        core::str::from_utf8(name_bytes).unwrap_or("unnamed")
    } else {
        "unnamed"
    };

    let options = SpawnOptions::new(name);

    match spawn_process(elf_data, &options) {
        Ok(result) => {
            #[allow(clippy::cast_possible_wrap)]
            SyscallResult::ok(result.pid as i64)
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PROCESS_WAIT` — wait for a child process to exit.
///
/// `arg0`: child process ID.
///
/// Returns: exit code on success, negative error on failure.
///
/// If the child is still running, blocks the calling task until the
/// child exits.
pub fn sys_process_wait(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::pcb;

    let child_pid = args.arg0;
    // The parent PID — for now, use 0 (kernel) as the "current process".
    // TODO: get actual current process ID from the calling task's
    // process association.
    let parent_pid = 0;

    // Try to reap immediately.
    match pcb::try_reap(parent_pid, child_pid) {
        Ok(Some(exit_code)) => {
            #[allow(clippy::cast_possible_wrap)]
            return SyscallResult::ok(exit_code as i64);
        }
        Ok(None) => {
            // Child still running — register wait and block.
            let task_id = sched::current_task_id();
            if let Err(e) = pcb::set_wait_task(child_pid, task_id) {
                return SyscallResult::err(e);
            }
            sched::block_current();

            // Woken up — try to reap again.
            match pcb::try_reap(parent_pid, child_pid) {
                Ok(Some(exit_code)) => {
                    #[allow(clippy::cast_possible_wrap)]
                    SyscallResult::ok(exit_code as i64)
                }
                Ok(None) => {
                    // Shouldn't happen — we were woken because it became
                    // a zombie.  Return WouldBlock defensively.
                    SyscallResult::err(KernelError::WouldBlock)
                }
                Err(e) => SyscallResult::err(e),
            }
        }
        Err(e) => SyscallResult::err(e),
    }
}

/// `SYS_PROCESS_ID` — get the current process ID.
///
/// Returns: the calling process's PID, or 0 if the task isn't
/// associated with a process.
pub fn sys_process_id(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    use crate::proc::thread;

    let task_id = sched::current_task_id();
    let pid = thread::owner_process(task_id).unwrap_or(0);

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(pid as i64)
}

/// `SYS_CAP_QUERY` — query the calling process's capabilities.
///
/// Returns the number of valid capabilities held by the calling process.
///
/// This is a simple count query.  A future extension will support
/// filling a user-space buffer with detailed capability entries.
pub fn sys_cap_query(args: &SyscallArgs) -> SyscallResult {
    let _ = args;
    use crate::proc::{pcb, thread};

    let task_id = sched::current_task_id();
    let pid = thread::owner_process(task_id).unwrap_or(0);

    // PID 0 (kernel) has no per-process cap table.
    if pid == 0 {
        return SyscallResult::ok(0);
    }

    let count = pcb::cap_count(pid).unwrap_or(0);

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(count as i64)
}

/// `SYS_SET_EXCEPTION_HANDLER` — register a per-process exception handler.
///
/// `arg0`: handler function address, or 0 to unregister.
pub fn sys_set_exception_handler(args: &SyscallArgs) -> SyscallResult {
    use crate::proc::{exception, thread};

    let handler_addr = args.arg0;
    let task_id = sched::current_task_id();

    let pid = match thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => {
            return SyscallResult::err(KernelError::NoSuchProcess);
        }
    };

    exception::set_handler(pid, handler_addr);
    SyscallResult::ok(0)
}

/// `SYS_EXCEPTION_RETURN` — resume from an exception handler.
///
/// `arg0`: pointer to the `ExceptionContext` on the user stack.
///
/// Restores the saved CPU state and resumes execution.  This syscall
/// does NOT return to the caller — it modifies the SYSRET frame to
/// jump to the context's saved RIP.
///
/// Since this needs to modify the syscall frame, it's handled as a
/// special case in `syscall_handler_inner` (like exec).
pub fn sys_exception_return_with_frame(
    frame: &mut super::entry::SyscallFrame,
) -> i64 {
    use crate::proc::exception::ExceptionContext;

    let ctx_ptr = frame.arg0 as *const ExceptionContext;

    // TODO: validate that ctx_ptr is in the user address space.
    // For now, trust the pointer.

    // SAFETY: ctx_ptr was written by the kernel's exception dispatch
    // code onto the user stack.  The handler may have modified fields
    // (e.g., rip to skip the faulting instruction).
    let ctx = unsafe { &*ctx_ptr };

    // Restore the SYSRET frame from the exception context.
    frame.user_rip = ctx.rip;
    frame.user_rsp = ctx.rsp;
    frame.user_rflags = ctx.rflags;
    frame.arg0 = ctx.rdi;  // rdi
    frame.arg1 = ctx.rsi;  // rsi
    frame.arg2 = ctx.rdx;  // rdx
    frame.arg3 = ctx.r10;  // r10
    frame.arg4 = ctx.r8;   // r8
    frame.arg5 = ctx.r9;   // r9
    frame.rbx = ctx.rbx;
    frame.rbp = ctx.rbp;
    frame.r12 = ctx.r12;
    frame.r13 = ctx.r13;
    frame.r14 = ctx.r14;
    frame.r15 = ctx.r15;

    serial_println!(
        "[exception] Returning from exception handler to {:#x}",
        ctx.rip
    );

    // Return value in RAX (not meaningful — the restored rax from
    // the context won't be used since we're restoring from the frame).
    ctx.rax as i64
}

/// `SYS_PROCESS_KILL` — force-terminate a process.
///
/// `arg0`: target process ID.
/// `arg1`: exit code (i32, sign-extended to u64).
///
/// Authority: the caller must be the parent of the target, or PID 0.
/// Cannot kill PID 0 (kernel) or the calling process itself (use
/// SYS_EXIT instead).
///
/// Returns: number of threads killed.
pub fn sys_process_kill(args: &super::dispatch::SyscallArgs) -> super::dispatch::SyscallResult {
    use crate::proc::{pcb, thread};
    use super::dispatch::SyscallResult;

    let target_pid = args.arg0;
    #[allow(clippy::cast_possible_wrap)]
    let exit_code = args.arg1 as i32;

    // Can't kill PID 0 (kernel).
    if target_pid == 0 {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Get the caller's process ID.
    let task_id = sched::current_task_id();
    let caller_pid = thread::owner_process(task_id).unwrap_or(0);

    // Can't kill self — use SYS_EXIT instead.
    if target_pid == caller_pid {
        return SyscallResult::err(KernelError::InvalidArgument);
    }

    // Authority check: caller must be the target's parent, PID 0,
    // or hold a Process capability with DELETE rights for the target.
    let target_parent = match pcb::parent(target_pid) {
        Some(p) => p,
        None => return SyscallResult::err(KernelError::NoSuchProcess),
    };
    let has_parent_auth = caller_pid == 0 || caller_pid == target_parent;
    let has_cap_auth = pcb::has_capability_for(
        caller_pid,
        crate::cap::ResourceType::Process,
        target_pid,
        crate::cap::Rights::DELETE,
    );
    if !has_parent_auth && !has_cap_auth {
        return SyscallResult::err(KernelError::PermissionDenied);
    }

    // Check the process isn't already a zombie or gone.
    match pcb::state(target_pid) {
        Some(pcb::ProcessState::Zombie) => {
            return SyscallResult::err(KernelError::ProcessExited);
        }
        None => {
            return SyscallResult::err(KernelError::NoSuchProcess);
        }
        _ => {}
    }

    // Set the exit code before killing threads so the zombie
    // transition has the correct code.
    if let Err(e) = pcb::set_exit_code(target_pid, exit_code) {
        return SyscallResult::err(e);
    }

    // Kill all threads in the target process.
    let killed = thread::kill_process_threads(target_pid);

    serial_println!(
        "[proc] Process {} killed by {} ({} threads, exit_code={})",
        target_pid, caller_pid, killed, exit_code
    );

    #[allow(clippy::cast_possible_wrap)]
    SyscallResult::ok(killed as i64)
}

/// `SYS_PROCESS_EXEC` — replace the current process image.
///
/// This handler receives the full `SyscallFrame` (not just args) because
/// on success it must rewrite the saved user RIP and RSP so that when
/// the SYSRET assembly path runs, it jumps to the new binary's entry
/// point with a fresh stack.
///
/// `frame.arg0`: pointer to ELF data in user memory.
/// `frame.arg1`: length of the ELF data (bytes).
///
/// On success: returns 0 in RAX, with user_rip and user_rsp in the
/// frame modified to point at the new binary.  All other saved
/// registers are zeroed (clean slate for the new binary).
///
/// On failure: returns a negative error code.  If the failure happens
/// after the old address space was torn down, the process is in a
/// broken state and should be killed.
pub fn sys_process_exec_with_frame(
    frame: &mut super::entry::SyscallFrame,
) -> i64 {
    use crate::proc::spawn::exec_process;
    use crate::proc::thread;

    let elf_ptr = frame.arg0 as usize;
    let elf_len = frame.arg1 as usize;

    // Validate arguments.
    if elf_len == 0 {
        return KernelError::InvalidArgument.code() as i64;
    }

    // Get the calling process's PID.
    let task_id = sched::current_task_id();
    let pid = match thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => {
            serial_println!("[exec] Task {} has no owning process", task_id);
            return KernelError::NoSuchProcess.code() as i64;
        }
    };

    // Read the ELF data from userspace.
    //
    // SAFETY: The pointer comes from the calling process's address
    // space, which is still intact at this point (we haven't torn it
    // down yet).  We copy the data into a kernel buffer before
    // clearing the address space.
    //
    // TODO: proper userspace pointer validation (bounds check against
    // the process's VMAs).
    let elf_data = unsafe {
        core::slice::from_raw_parts(elf_ptr as *const u8, elf_len)
    };

    // We need to copy the ELF data into a kernel buffer BEFORE we
    // tear down the user address space (which would unmap the source).
    let elf_copy = alloc::vec::Vec::from(elf_data);

    // Exec: validate ELF, tear down old AS, load new AS, set up stack.
    match exec_process(pid, &elf_copy) {
        Ok(result) => {
            // Success: rewrite the saved frame so SYSRET returns to the
            // new entry point with a fresh stack and clean registers.
            frame.user_rip = result.entry_rip;
            frame.user_rsp = result.user_rsp;

            // Zero all saved general-purpose registers — the new binary
            // starts with a clean slate.
            frame.arg0 = 0;  // rdi
            frame.arg1 = 0;  // rsi
            frame.arg2 = 0;  // rdx
            frame.arg3 = 0;  // r10
            frame.arg4 = 0;  // r8
            frame.arg5 = 0;  // r9
            frame.rbx = 0;
            frame.rbp = 0;
            frame.r12 = 0;
            frame.r13 = 0;
            frame.r14 = 0;
            frame.r15 = 0;

            // RFLAGS: keep IF=1 (interrupts enabled), reserved bit 1.
            // Clear everything else (DF, TF, etc.).
            frame.user_rflags = 0x202;

            serial_println!(
                "[exec] Process {} exec successful — returning to {:#x}",
                pid, result.entry_rip
            );
            0 // Success (returned in RAX).
        }
        Err(e) => {
            serial_println!(
                "[exec] Process {} exec failed: {:?} — process may be broken",
                pid, e
            );
            e.code() as i64
        }
    }
}
