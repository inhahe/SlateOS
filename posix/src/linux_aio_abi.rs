//! `<linux/aio_abi.h>` — kernel asynchronous I/O (AIO) interface.
//!
//! This is the kernel-level AIO interface (via `io_setup`, `io_submit`,
//! `io_getevents`, `io_destroy` syscalls), distinct from the POSIX AIO
//! functions (`aio_read`, `aio_write`). Modern code should prefer
//! io_uring, but kernel AIO is still used for O_DIRECT I/O.

use crate::errno;

// ---------------------------------------------------------------------------
// AIO commands (iocb.aio_lio_opcode)
// ---------------------------------------------------------------------------

/// Read operation.
pub const IOCB_CMD_PREAD: u16 = 0;
/// Write operation.
pub const IOCB_CMD_PWRITE: u16 = 1;
/// Fsync.
pub const IOCB_CMD_FSYNC: u16 = 2;
/// Fdatasync.
pub const IOCB_CMD_FDSYNC: u16 = 3;
/// Vectored read.
pub const IOCB_CMD_PREADV: u16 = 7;
/// Vectored write.
pub const IOCB_CMD_PWRITEV: u16 = 8;
/// No-op (poll).
pub const IOCB_CMD_NOOP: u16 = 6;

// ---------------------------------------------------------------------------
// AIO flags (iocb.aio_flags)
// ---------------------------------------------------------------------------

/// Set if using eventfd for notification.
pub const IOCB_FLAG_RESFD: u32 = 1 << 0;
/// Submit as IOPRIO class.
pub const IOCB_FLAG_IOPRIO: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// I/O control block
// ---------------------------------------------------------------------------

/// Kernel AIO I/O control block (64 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Iocb {
    /// Data returned in io_event.
    pub aio_data: u64,
    /// PADDED(aio_key, aio_rw_flags).
    pub aio_key: u32,
    /// Per-I/O RWF_* flags.
    pub aio_rw_flags: u32,
    /// Operation (IOCB_CMD_*).
    pub aio_lio_opcode: u16,
    /// Request priority.
    pub aio_reqprio: i16,
    /// File descriptor.
    pub aio_fildes: u32,
    /// Buffer address.
    pub aio_buf: u64,
    /// Number of bytes.
    pub aio_nbytes: u64,
    /// File offset.
    pub aio_offset: i64,
    /// Reserved.
    _reserved2: u64,
    /// Flags (IOCB_FLAG_*).
    pub aio_flags: u32,
    /// eventfd for signal completion.
    pub aio_resfd: u32,
}

impl Iocb {
    /// Create a zeroed I/O control block.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Completion event (32 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoEvent {
    /// Data from iocb.
    pub data: u64,
    /// iocb address.
    pub obj: u64,
    /// Result (bytes transferred or negative errno).
    pub res: i64,
    /// Secondary result.
    pub res2: i64,
}

impl IoEvent {
    /// Create a zeroed I/O event.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Stubs
// ---------------------------------------------------------------------------

/// Create an AIO context.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_setup(_nr_events: u32, _ctx_idp: *mut u64) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Destroy an AIO context.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_destroy(_ctx_id: u64) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Submit AIO requests.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_submit(_ctx_id: u64, _nr: i64, _iocbpp: *mut *mut Iocb) -> i64 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Get AIO completion events.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_getevents(
    _ctx_id: u64,
    _min_nr: i64,
    _nr: i64,
    _events: *mut IoEvent,
    _timeout: *mut u8,
) -> i64 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iocb_size() {
        assert_eq!(core::mem::size_of::<Iocb>(), 64);
    }

    #[test]
    fn test_io_event_size() {
        assert_eq!(core::mem::size_of::<IoEvent>(), 32);
    }

    #[test]
    fn test_iocb_zeroed() {
        let iocb = Iocb::zeroed();
        assert_eq!(iocb.aio_data, 0);
        assert_eq!(iocb.aio_lio_opcode, 0);
        assert_eq!(iocb.aio_fildes, 0);
        assert_eq!(iocb.aio_buf, 0);
        assert_eq!(iocb.aio_nbytes, 0);
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            IOCB_CMD_PREAD, IOCB_CMD_PWRITE, IOCB_CMD_FSYNC,
            IOCB_CMD_FDSYNC, IOCB_CMD_NOOP, IOCB_CMD_PREADV,
            IOCB_CMD_PWRITEV,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags() {
        assert_eq!(IOCB_FLAG_RESFD, 1);
        assert_eq!(IOCB_FLAG_IOPRIO, 2);
        assert_eq!(IOCB_FLAG_RESFD & IOCB_FLAG_IOPRIO, 0);
    }

    #[test]
    fn test_io_setup_stub() {
        let mut ctx: u64 = 0;
        let ret = io_setup(1, &mut ctx as *mut u64);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_io_destroy_stub() {
        let ret = io_destroy(0);
        assert_eq!(ret, -1);
    }
}
