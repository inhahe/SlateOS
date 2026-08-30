//! `<linux/if_xdp.h>` — AF_XDP socket and UMEM ring ABI.
//!
//! AF_XDP delivers DPDK-class packet throughput inside the Linux
//! kernel: a userspace UMEM buffer pool plus four lockless rings
//! (fill, completion, rx, tx) shared with the driver. The constants
//! here describe socket options, ring offsets, and frame descriptors.

// ---------------------------------------------------------------------------
// Socket address family
// ---------------------------------------------------------------------------

/// `AF_XDP` (PF_XDP) — same value on every Linux arch.
pub const AF_XDP: u32 = 44;
/// SOL level for AF_XDP setsockopt.
pub const SOL_XDP: u32 = 283;

// ---------------------------------------------------------------------------
// XDP_* setsockopt names
// ---------------------------------------------------------------------------

pub const XDP_MMAP_OFFSETS: u32 = 1;
pub const XDP_RX_RING: u32 = 2;
pub const XDP_TX_RING: u32 = 3;
pub const XDP_UMEM_REG: u32 = 4;
pub const XDP_UMEM_FILL_RING: u32 = 5;
pub const XDP_UMEM_COMPLETION_RING: u32 = 6;
pub const XDP_STATISTICS: u32 = 7;
pub const XDP_OPTIONS: u32 = 8;

// ---------------------------------------------------------------------------
// `struct sockaddr_xdp.sxdp_flags` and bind flags
// ---------------------------------------------------------------------------

pub const XDP_SHARED_UMEM: u16 = 1 << 0;
pub const XDP_COPY: u16 = 1 << 1;
pub const XDP_ZEROCOPY: u16 = 1 << 2;
pub const XDP_USE_NEED_WAKEUP: u16 = 1 << 3;
pub const XDP_USE_SG: u16 = 1 << 4;

// ---------------------------------------------------------------------------
// XDP_OPTIONS replies (bits)
// ---------------------------------------------------------------------------

pub const XDP_OPTIONS_ZEROCOPY: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// UMEM registration flags (struct xdp_umem_reg.flags)
// ---------------------------------------------------------------------------

pub const XDP_UMEM_UNALIGNED_CHUNK_FLAG: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// `struct xdp_desc.options` flags
// ---------------------------------------------------------------------------

pub const XDP_PKT_CONTD: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Ring mmap pgoff constants
// ---------------------------------------------------------------------------

pub const XDP_PGOFF_RX_RING: u64 = 0;
pub const XDP_PGOFF_TX_RING: u64 = 0x80000000;
pub const XDP_UMEM_PGOFF_FILL_RING: u64 = 0x100000000;
pub const XDP_UMEM_PGOFF_COMPLETION_RING: u64 = 0x180000000;

// ---------------------------------------------------------------------------
// Default frame sizes
// ---------------------------------------------------------------------------

pub const XSK_UMEM__DEFAULT_FRAME_SHIFT: u32 = 12;
pub const XSK_UMEM__DEFAULT_FRAME_SIZE: u32 = 1 << XSK_UMEM__DEFAULT_FRAME_SHIFT;
pub const XSK_UMEM__DEFAULT_FRAME_HEADROOM: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_family() {
        // AF_XDP=44, SOL_XDP=283 are the Linux-wide constants.
        assert_eq!(AF_XDP, 44);
        assert_eq!(SOL_XDP, 283);
    }

    #[test]
    fn test_sockopts_dense_1_to_8() {
        let o = [
            XDP_MMAP_OFFSETS,
            XDP_RX_RING,
            XDP_TX_RING,
            XDP_UMEM_REG,
            XDP_UMEM_FILL_RING,
            XDP_UMEM_COMPLETION_RING,
            XDP_STATISTICS,
            XDP_OPTIONS,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_bind_flags_pow2() {
        for &b in &[
            XDP_SHARED_UMEM,
            XDP_COPY,
            XDP_ZEROCOPY,
            XDP_USE_NEED_WAKEUP,
            XDP_USE_SG,
        ] {
            assert!(b.is_power_of_two());
        }
        // COPY and ZEROCOPY are mutually exclusive but both individually
        // valid — they are different bits.
        assert_ne!(XDP_COPY, XDP_ZEROCOPY);
    }

    #[test]
    fn test_ring_pgoffs_distinct() {
        let p = [
            XDP_PGOFF_RX_RING,
            XDP_PGOFF_TX_RING,
            XDP_UMEM_PGOFF_FILL_RING,
            XDP_UMEM_PGOFF_COMPLETION_RING,
        ];
        for i in 0..p.len() {
            for j in (i + 1)..p.len() {
                assert_ne!(p[i], p[j]);
            }
        }
        // Monotonic — each ring lives at a higher offset than the previous.
        assert!(XDP_PGOFF_RX_RING < XDP_PGOFF_TX_RING);
        assert!(XDP_PGOFF_TX_RING < XDP_UMEM_PGOFF_FILL_RING);
        assert!(XDP_UMEM_PGOFF_FILL_RING < XDP_UMEM_PGOFF_COMPLETION_RING);
    }

    #[test]
    fn test_default_frame_size_is_page() {
        // 4 KiB = 2^12.
        assert_eq!(XSK_UMEM__DEFAULT_FRAME_SIZE, 4096);
        assert_eq!(
            XSK_UMEM__DEFAULT_FRAME_SIZE,
            1u32 << XSK_UMEM__DEFAULT_FRAME_SHIFT
        );
    }

    #[test]
    fn test_options_and_umem_flags_pow2() {
        assert!(XDP_OPTIONS_ZEROCOPY.is_power_of_two());
        assert!(XDP_UMEM_UNALIGNED_CHUNK_FLAG.is_power_of_two());
        assert!(XDP_PKT_CONTD.is_power_of_two());
    }
}
