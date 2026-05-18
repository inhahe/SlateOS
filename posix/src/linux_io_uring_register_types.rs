//! `<linux/io_uring.h>` — io_uring_register() operation constants.
//!
//! The `io_uring_register()` syscall registers resources (file
//! descriptors, buffers, event fds) with the io_uring instance for
//! faster access during I/O operations. These opcodes identify the
//! registration operation to perform.

// ---------------------------------------------------------------------------
// io_uring_register() opcodes
// ---------------------------------------------------------------------------

/// Register a set of file descriptors.
pub const IORING_REGISTER_BUFFERS: u32 = 0;
/// Unregister previously registered buffers.
pub const IORING_UNREGISTER_BUFFERS: u32 = 1;
/// Register a set of file descriptors.
pub const IORING_REGISTER_FILES: u32 = 2;
/// Unregister previously registered files.
pub const IORING_UNREGISTER_FILES: u32 = 3;
/// Register eventfd for CQ notifications.
pub const IORING_REGISTER_EVENTFD: u32 = 4;
/// Unregister eventfd.
pub const IORING_UNREGISTER_EVENTFD: u32 = 5;
/// Update registered file descriptors.
pub const IORING_REGISTER_FILES_UPDATE: u32 = 6;
/// Register eventfd (async only).
pub const IORING_REGISTER_EVENTFD_ASYNC: u32 = 7;
/// Probe supported opcodes.
pub const IORING_REGISTER_PROBE: u32 = 8;
/// Register personality credentials.
pub const IORING_REGISTER_PERSONALITY: u32 = 9;
/// Unregister personality.
pub const IORING_UNREGISTER_PERSONALITY: u32 = 10;
/// Register io-wq restrictions.
pub const IORING_REGISTER_RESTRICTIONS: u32 = 11;
/// Enable a disabled ring.
pub const IORING_REGISTER_ENABLE_RINGS: u32 = 12;
/// Register buffers with tags.
pub const IORING_REGISTER_BUFFERS2: u32 = 15;
/// Update tagged buffers.
pub const IORING_REGISTER_BUFFERS_UPDATE: u32 = 16;
/// Register files with tags.
pub const IORING_REGISTER_FILES2: u32 = 17;
/// Update tagged files.
pub const IORING_REGISTER_FILES_UPDATE2: u32 = 18;
/// Register io-wq max workers.
pub const IORING_REGISTER_IOWQ_AFF: u32 = 19;
/// Unregister io-wq affinity.
pub const IORING_UNREGISTER_IOWQ_AFF: u32 = 20;
/// Set io-wq max unbound workers.
pub const IORING_REGISTER_IOWQ_MAX_WORKERS: u32 = 21;
/// Register ring with kernel (for multishot).
pub const IORING_REGISTER_RING_FDS: u32 = 22;
/// Unregister ring fds.
pub const IORING_UNREGISTER_RING_FDS: u32 = 23;
/// Register provided buffer ring.
pub const IORING_REGISTER_PBUF_RING: u32 = 24;
/// Unregister provided buffer ring.
pub const IORING_UNREGISTER_PBUF_RING: u32 = 25;
/// Sync cancel operation.
pub const IORING_REGISTER_SYNC_CANCEL: u32 = 26;
/// Register file alloc range.
pub const IORING_REGISTER_FILE_ALLOC_RANGE: u32 = 27;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_opcodes_distinct() {
        let ops = [
            IORING_REGISTER_BUFFERS, IORING_UNREGISTER_BUFFERS,
            IORING_REGISTER_FILES, IORING_UNREGISTER_FILES,
            IORING_REGISTER_EVENTFD, IORING_UNREGISTER_EVENTFD,
            IORING_REGISTER_FILES_UPDATE, IORING_REGISTER_EVENTFD_ASYNC,
            IORING_REGISTER_PROBE, IORING_REGISTER_PERSONALITY,
            IORING_UNREGISTER_PERSONALITY, IORING_REGISTER_RESTRICTIONS,
            IORING_REGISTER_ENABLE_RINGS, IORING_REGISTER_BUFFERS2,
            IORING_REGISTER_BUFFERS_UPDATE, IORING_REGISTER_FILES2,
            IORING_REGISTER_FILES_UPDATE2, IORING_REGISTER_IOWQ_AFF,
            IORING_UNREGISTER_IOWQ_AFF, IORING_REGISTER_IOWQ_MAX_WORKERS,
            IORING_REGISTER_RING_FDS, IORING_UNREGISTER_RING_FDS,
            IORING_REGISTER_PBUF_RING, IORING_UNREGISTER_PBUF_RING,
            IORING_REGISTER_SYNC_CANCEL, IORING_REGISTER_FILE_ALLOC_RANGE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_register_buffers_is_zero() {
        assert_eq!(IORING_REGISTER_BUFFERS, 0);
    }

    #[test]
    fn test_register_unregister_pairs() {
        assert_eq!(IORING_REGISTER_BUFFERS + 1, IORING_UNREGISTER_BUFFERS);
        assert_eq!(IORING_REGISTER_FILES + 1, IORING_UNREGISTER_FILES);
        assert_eq!(IORING_REGISTER_EVENTFD + 1, IORING_UNREGISTER_EVENTFD);
    }

    #[test]
    fn test_probe_opcode() {
        assert_eq!(IORING_REGISTER_PROBE, 8);
    }
}
