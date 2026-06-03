//! `<linux/if_xdp.h>` — AF_XDP socket ABI.
//!
//! AF_XDP is the kernel's high-throughput, low-overhead packet IO
//! interface. Userspace sets up a UMEM (a chunk of memory shared
//! with the kernel), four rings (RX/TX/FILL/COMPLETION), and pumps
//! packets straight from the NIC into the UMEM without copies.

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

pub const AF_XDP: u32 = 44;
pub const PF_XDP: u32 = AF_XDP;

// ---------------------------------------------------------------------------
// `sockaddr_xdp.sxdp_flags`
// ---------------------------------------------------------------------------

pub const XDP_SHARED_UMEM: u16 = 1 << 0;
pub const XDP_COPY: u16 = 1 << 1;
pub const XDP_ZEROCOPY: u16 = 1 << 2;
pub const XDP_USE_NEED_WAKEUP: u16 = 1 << 3;
pub const XDP_USE_SG: u16 = 1 << 4;

// ---------------------------------------------------------------------------
// `setsockopt(SOL_XDP, …)` options
// ---------------------------------------------------------------------------

pub const SOL_XDP: u32 = 283;

pub const XDP_MMAP_OFFSETS: u32 = 1;
pub const XDP_RX_RING: u32 = 2;
pub const XDP_TX_RING: u32 = 3;
pub const XDP_UMEM_REG: u32 = 4;
pub const XDP_UMEM_FILL_RING: u32 = 5;
pub const XDP_UMEM_COMPLETION_RING: u32 = 6;
pub const XDP_STATISTICS: u32 = 7;
pub const XDP_OPTIONS: u32 = 8;

// ---------------------------------------------------------------------------
// `mmap()` page offsets for each ring
// ---------------------------------------------------------------------------

pub const XDP_PGOFF_RX_RING: u64 = 0;
pub const XDP_PGOFF_TX_RING: u64 = 0x8000_0000;
pub const XDP_UMEM_PGOFF_FILL_RING: u64 = 0x1_0000_0000;
pub const XDP_UMEM_PGOFF_COMPLETION_RING: u64 = 0x1_8000_0000;

// ---------------------------------------------------------------------------
// XDP program action codes (returned by BPF `XDP_*` programs)
// ---------------------------------------------------------------------------

pub const XDP_ABORTED: u32 = 0;
pub const XDP_DROP: u32 = 1;
pub const XDP_PASS: u32 = 2;
pub const XDP_TX: u32 = 3;
pub const XDP_REDIRECT: u32 = 4;

// ---------------------------------------------------------------------------
// `xdp_options_set` flags
// ---------------------------------------------------------------------------

pub const XDP_OPTIONS_ZEROCOPY: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// UMEM chunk-size limits
// ---------------------------------------------------------------------------

pub const XDP_UMEM_MIN_CHUNK_SIZE: u32 = 2048;
pub const XDP_UMEM_MAX_CHUNK_SIZE: u32 = 65_536;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_xdp_is_44() {
        // AF_XDP got assigned 44 when it merged in Linux 4.18.
        assert_eq!(AF_XDP, 44);
        assert_eq!(PF_XDP, AF_XDP);
    }

    #[test]
    fn test_sxdp_flags_dense_low_5_bits() {
        let f = [XDP_SHARED_UMEM, XDP_COPY, XDP_ZEROCOPY, XDP_USE_NEED_WAKEUP, XDP_USE_SG];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v, 1 << i);
        }
        let mut or = 0u16;
        for v in f {
            or |= v;
        }
        assert_eq!(or, 0x1F);
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
        // SOL_XDP is the level all of these are set on.
        assert_eq!(SOL_XDP, 283);
    }

    #[test]
    fn test_mmap_offsets_separated() {
        // Each ring sits at a distinct 2-GiB-aligned slice of the
        // mmap pseudo-file so userspace can map them all by adding
        // a known offset to a single fd.
        assert_eq!(XDP_PGOFF_RX_RING, 0);
        assert_eq!(XDP_PGOFF_TX_RING, 0x8000_0000);
        assert_eq!(XDP_UMEM_PGOFF_FILL_RING, 0x1_0000_0000);
        assert_eq!(XDP_UMEM_PGOFF_COMPLETION_RING, 0x1_8000_0000);
        // The gap between adjacent rings is constant (2 GiB).
        assert_eq!(
            XDP_UMEM_PGOFF_COMPLETION_RING - XDP_UMEM_PGOFF_FILL_RING,
            0x8000_0000
        );
    }

    #[test]
    fn test_xdp_actions_dense_0_to_4() {
        let a = [XDP_ABORTED, XDP_DROP, XDP_PASS, XDP_TX, XDP_REDIRECT];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_umem_chunk_size_bounds() {
        // Min = 2K (typical L2 frame), max = 64K (jumbo + headroom).
        assert_eq!(XDP_UMEM_MIN_CHUNK_SIZE, 2048);
        assert_eq!(XDP_UMEM_MAX_CHUNK_SIZE, 65_536);
        // Both powers of two.
        assert!(XDP_UMEM_MIN_CHUNK_SIZE.is_power_of_two());
        assert!(XDP_UMEM_MAX_CHUNK_SIZE.is_power_of_two());
    }
}
