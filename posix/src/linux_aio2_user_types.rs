//! Continuation of `aio` — kernel ring-buffer layout used by `libaio`.
//!
//! `libaio` exposes the kernel's per-context ring buffer directly via
//! mmap so userspace can reap events without syscalls in the fast
//! path. The layout is defined by `struct aio_ring` in
//! `fs/aio.c` and re-exposed to userspace as a stable ABI.

// ---------------------------------------------------------------------------
// `struct aio_ring` field offsets (header is 32 bytes)
// ---------------------------------------------------------------------------

pub const AIO_RING_OFF_ID: usize = 0;
pub const AIO_RING_OFF_NR: usize = 4;
pub const AIO_RING_OFF_HEAD: usize = 8;
pub const AIO_RING_OFF_TAIL: usize = 12;
pub const AIO_RING_OFF_MAGIC: usize = 16;
pub const AIO_RING_OFF_COMPAT_FEATURES: usize = 20;
pub const AIO_RING_OFF_INCOMPAT_FEATURES: usize = 24;
pub const AIO_RING_OFF_HEADER_LENGTH: usize = 28;

pub const AIO_RING_HEADER_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// Feature bits (`compat_features` / `incompat_features`)
// ---------------------------------------------------------------------------

pub const AIO_RING_COMPAT_FEATURE_MULTI_PAGE: u32 = 0x01;

/// No incompat features are presently defined; reserved 0 means
/// userspace may safely consume the ring.
pub const AIO_RING_INCOMPAT_FEATURES_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Event slot ("io_event") on the wire — 32 bytes
// ---------------------------------------------------------------------------

pub const IO_EVENT_SIZE: usize = 32;
pub const IO_EVENT_OFF_DATA: usize = 0; // u64
pub const IO_EVENT_OFF_OBJ: usize = 8; // u64 (iocb pointer)
pub const IO_EVENT_OFF_RES: usize = 16; // i64
pub const IO_EVENT_OFF_RES2: usize = 24; // i64

// ---------------------------------------------------------------------------
// IOCB on the wire — fixed 64-byte layout
// ---------------------------------------------------------------------------

pub const IOCB_SIZE: usize = 64;
pub const IOCB_OFF_DATA: usize = 0;
pub const IOCB_OFF_KEY: usize = 8;
pub const IOCB_OFF_AIO_RW_FLAGS: usize = 12;
pub const IOCB_OFF_LIO_OPCODE: usize = 16;
pub const IOCB_OFF_REQPRIO: usize = 18;
pub const IOCB_OFF_FILDES: usize = 20;
pub const IOCB_OFF_BUF: usize = 24;
pub const IOCB_OFF_NBYTES: usize = 32;
pub const IOCB_OFF_OFFSET: usize = 40;
pub const IOCB_OFF_RESERVED2: usize = 48;
pub const IOCB_OFF_FLAGS: usize = 56;
pub const IOCB_OFF_RESFD: usize = 60;

// ---------------------------------------------------------------------------
// RW flags (`aio_rw_flags`) — same bits as `pwritev2(2)` RWF_*
// ---------------------------------------------------------------------------

pub const AIO_RWF_HIPRI: u32 = 0x00000001;
pub const AIO_RWF_DSYNC: u32 = 0x00000002;
pub const AIO_RWF_SYNC: u32 = 0x00000004;
pub const AIO_RWF_NOWAIT: u32 = 0x00000008;
pub const AIO_RWF_APPEND: u32 = 0x00000010;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aio_ring_offsets_dense() {
        // Each field is u32 (4 bytes); offsets are 0,4,8,...,28.
        let o = [
            AIO_RING_OFF_ID,
            AIO_RING_OFF_NR,
            AIO_RING_OFF_HEAD,
            AIO_RING_OFF_TAIL,
            AIO_RING_OFF_MAGIC,
            AIO_RING_OFF_COMPAT_FEATURES,
            AIO_RING_OFF_INCOMPAT_FEATURES,
            AIO_RING_OFF_HEADER_LENGTH,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v, i * 4);
        }
        assert_eq!(AIO_RING_HEADER_SIZE, 32);
    }

    #[test]
    fn test_feature_bits_initial() {
        assert!(AIO_RING_COMPAT_FEATURE_MULTI_PAGE.is_power_of_two());
        assert_eq!(AIO_RING_INCOMPAT_FEATURES_NONE, 0);
    }

    #[test]
    fn test_io_event_layout() {
        // 4 u64 fields back-to-back = 32 bytes.
        assert_eq!(IO_EVENT_SIZE, 32);
        assert_eq!(IO_EVENT_OFF_DATA, 0);
        assert_eq!(IO_EVENT_OFF_OBJ, 8);
        assert_eq!(IO_EVENT_OFF_RES, 16);
        assert_eq!(IO_EVENT_OFF_RES2, 24);
    }

    #[test]
    fn test_iocb_layout_64_bytes() {
        assert_eq!(IOCB_SIZE, 64);
        // Each subsequent offset is strictly greater.
        let o = [
            IOCB_OFF_DATA,
            IOCB_OFF_KEY,
            IOCB_OFF_AIO_RW_FLAGS,
            IOCB_OFF_LIO_OPCODE,
            IOCB_OFF_REQPRIO,
            IOCB_OFF_FILDES,
            IOCB_OFF_BUF,
            IOCB_OFF_NBYTES,
            IOCB_OFF_OFFSET,
            IOCB_OFF_RESERVED2,
            IOCB_OFF_FLAGS,
            IOCB_OFF_RESFD,
        ];
        for w in o.windows(2) {
            assert!(w[0] < w[1]);
        }
        // Last field starts before end of struct.
        assert!(IOCB_OFF_RESFD < IOCB_SIZE);
    }

    #[test]
    fn test_rwf_bits_low_5() {
        let f = [
            AIO_RWF_HIPRI,
            AIO_RWF_DSYNC,
            AIO_RWF_SYNC,
            AIO_RWF_NOWAIT,
            AIO_RWF_APPEND,
        ];
        let mut or = 0u32;
        for v in f {
            assert!(v.is_power_of_two());
            or |= v;
        }
        assert_eq!(or, 0x1F);
    }
}
