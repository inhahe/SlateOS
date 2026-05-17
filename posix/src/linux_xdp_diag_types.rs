//! `<linux/xdp_diag.h>` — XDP (eXpress Data Path) socket diagnostics constants.
//!
//! The XDP socket (AF_XDP) diagnostics interface allows tools to
//! inspect XDP socket state via netlink. AF_XDP sockets provide
//! kernel-bypass packet I/O by mapping UMEM (user-space memory) into
//! the NIC's DMA ring buffers. The diag interface reports ring sizes,
//! UMEM configuration, queue bindings, and statistics. Used for
//! debugging high-performance AF_XDP applications (DPDK-like).

// ---------------------------------------------------------------------------
// XDP diag show flags
// ---------------------------------------------------------------------------

/// Show XDP socket info.
pub const XDP_SHOW_INFO: u32 = 1 << 0;
/// Show ring info (RX/TX/fill/completion sizes).
pub const XDP_SHOW_RING_CFG: u32 = 1 << 1;
/// Show UMEM (user-space memory) info.
pub const XDP_SHOW_UMEM: u32 = 1 << 2;
/// Show memory info.
pub const XDP_SHOW_MEMINFO: u32 = 1 << 3;
/// Show statistics.
pub const XDP_SHOW_STATS: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// XDP diag response attributes
// ---------------------------------------------------------------------------

/// XDP socket info.
pub const XDP_DIAG_INFO: u32 = 0;
/// UID of socket owner.
pub const XDP_DIAG_UID: u32 = 1;
/// RX ring size.
pub const XDP_DIAG_RX_RING: u32 = 2;
/// TX ring size.
pub const XDP_DIAG_TX_RING: u32 = 3;
/// UMEM configuration.
pub const XDP_DIAG_UMEM: u32 = 4;
/// Fill ring size.
pub const XDP_DIAG_UMEM_FILL_RING: u32 = 5;
/// Completion ring size.
pub const XDP_DIAG_UMEM_COMPLETION_RING: u32 = 6;
/// Memory info.
pub const XDP_DIAG_MEMINFO: u32 = 7;
/// Statistics.
pub const XDP_DIAG_STATS: u32 = 8;

// ---------------------------------------------------------------------------
// XDP socket options (SOL_XDP level)
// ---------------------------------------------------------------------------

/// XDP socket option level.
pub const SOL_XDP: u32 = 283;
/// Set/get RX ring size.
pub const XDP_RX_RING: u32 = 1;
/// Set/get TX ring size.
pub const XDP_TX_RING: u32 = 2;
/// Set/get UMEM registration.
pub const XDP_UMEM_REG: u32 = 3;
/// Set/get UMEM fill ring size.
pub const XDP_UMEM_FILL_RING: u32 = 4;
/// Set/get UMEM completion ring size.
pub const XDP_UMEM_COMPLETION_RING: u32 = 5;
/// Get per-socket statistics.
pub const XDP_STATISTICS: u32 = 7;
/// Get mmap offsets.
pub const XDP_MMAP_OFFSETS: u32 = 1;
/// Get options.
pub const XDP_OPTIONS: u32 = 8;

// ---------------------------------------------------------------------------
// XDP bind flags
// ---------------------------------------------------------------------------

/// Shared UMEM (share with another socket).
pub const XDP_SHARED_UMEM: u32 = 1 << 0;
/// Copy mode (use kernel copy, not zero-copy).
pub const XDP_COPY: u32 = 1 << 1;
/// Zero-copy mode.
pub const XDP_ZEROCOPY: u32 = 1 << 2;
/// Use need-wakeup feature.
pub const XDP_USE_NEED_WAKEUP: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// UMEM flags
// ---------------------------------------------------------------------------

/// Use huge pages for UMEM.
pub const XDP_UMEM_UNALIGNED_CHUNK_FLAG: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_show_flags_no_overlap() {
        let flags = [
            XDP_SHOW_INFO, XDP_SHOW_RING_CFG,
            XDP_SHOW_UMEM, XDP_SHOW_MEMINFO, XDP_SHOW_STATS,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_diag_attrs_distinct() {
        let attrs = [
            XDP_DIAG_INFO, XDP_DIAG_UID,
            XDP_DIAG_RX_RING, XDP_DIAG_TX_RING,
            XDP_DIAG_UMEM, XDP_DIAG_UMEM_FILL_RING,
            XDP_DIAG_UMEM_COMPLETION_RING,
            XDP_DIAG_MEMINFO, XDP_DIAG_STATS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_bind_flags_no_overlap() {
        let flags = [
            XDP_SHARED_UMEM, XDP_COPY,
            XDP_ZEROCOPY, XDP_USE_NEED_WAKEUP,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_sol_xdp() {
        assert_eq!(SOL_XDP, 283);
    }

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            XDP_RX_RING, XDP_TX_RING, XDP_UMEM_REG,
            XDP_UMEM_FILL_RING, XDP_UMEM_COMPLETION_RING,
            XDP_STATISTICS, XDP_OPTIONS,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}
