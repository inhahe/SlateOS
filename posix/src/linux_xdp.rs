//! `<linux/if_xdp.h>` — XDP (eXpress Data Path) socket constants.
//!
//! AF_XDP sockets (XSK) provide a high-performance path for
//! userspace packet processing, bypassing most of the kernel
//! network stack. Packets are delivered via shared UMEM ring
//! buffers for zero-copy operation.

// ---------------------------------------------------------------------------
// XDP socket options (SOL_XDP)
// ---------------------------------------------------------------------------

/// Bind XDP socket to a queue.
pub const XDP_MMAP_OFFSETS: u32 = 1;
/// RX ring setup.
pub const XDP_RX_RING: u32 = 2;
/// TX ring setup.
pub const XDP_TX_RING: u32 = 3;
/// UMEM registration.
pub const XDP_UMEM_REG: u32 = 4;
/// Fill ring setup.
pub const XDP_UMEM_FILL_RING: u32 = 5;
/// Completion ring setup.
pub const XDP_UMEM_COMPLETION_RING: u32 = 6;
/// Statistics.
pub const XDP_STATISTICS: u32 = 7;
/// Socket options.
pub const XDP_OPTIONS: u32 = 8;

// ---------------------------------------------------------------------------
// XDP bind flags
// ---------------------------------------------------------------------------

/// Share UMEM between sockets.
pub const XDP_SHARED_UMEM: u16 = 1 << 0;
/// Copy mode (no zero-copy).
pub const XDP_COPY: u16 = 1 << 1;
/// Zero-copy mode.
pub const XDP_ZEROCOPY: u16 = 1 << 2;
/// Use need_wakeup flag optimization.
pub const XDP_USE_NEED_WAKEUP: u16 = 1 << 3;

// ---------------------------------------------------------------------------
// XDP ring flags
// ---------------------------------------------------------------------------

/// Ring needs wakeup (poll/sendto needed).
pub const XDP_RING_NEED_WAKEUP: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// XDP UMEM flags
// ---------------------------------------------------------------------------

/// Unaligned chunks mode.
pub const XDP_UMEM_UNALIGNED_CHUNK_FLAG: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// XDP action codes (return from XDP BPF program)
// ---------------------------------------------------------------------------

/// Abort (drop with error trace).
pub const XDP_ABORTED: u32 = 0;
/// Drop packet silently.
pub const XDP_DROP: u32 = 1;
/// Pass to normal network stack.
pub const XDP_PASS: u32 = 2;
/// Transmit back out same interface.
pub const XDP_TX: u32 = 3;
/// Redirect to another interface or socket.
pub const XDP_REDIRECT: u32 = 4;

// ---------------------------------------------------------------------------
// XDP attach modes
// ---------------------------------------------------------------------------

/// Generic XDP (software fallback).
pub const XDP_FLAGS_SKB_MODE: u32 = 1 << 1;
/// Native XDP (driver support).
pub const XDP_FLAGS_DRV_MODE: u32 = 1 << 2;
/// Hardware offload XDP.
pub const XDP_FLAGS_HW_MODE: u32 = 1 << 3;
/// Replace existing XDP program.
pub const XDP_FLAGS_REPLACE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sock_opts_distinct() {
        let opts = [
            XDP_MMAP_OFFSETS, XDP_RX_RING, XDP_TX_RING,
            XDP_UMEM_REG, XDP_UMEM_FILL_RING,
            XDP_UMEM_COMPLETION_RING, XDP_STATISTICS, XDP_OPTIONS,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_bind_flags_powers_of_two() {
        let flags = [XDP_SHARED_UMEM, XDP_COPY, XDP_ZEROCOPY, XDP_USE_NEED_WAKEUP];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_bind_flags_no_overlap() {
        let flags = [XDP_SHARED_UMEM, XDP_COPY, XDP_ZEROCOPY, XDP_USE_NEED_WAKEUP];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_actions_distinct() {
        let actions = [XDP_ABORTED, XDP_DROP, XDP_PASS, XDP_TX, XDP_REDIRECT];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_attach_flags_powers_of_two() {
        let flags = [
            XDP_FLAGS_SKB_MODE, XDP_FLAGS_DRV_MODE,
            XDP_FLAGS_HW_MODE, XDP_FLAGS_REPLACE,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_attach_flags_no_overlap() {
        let flags = [
            XDP_FLAGS_SKB_MODE, XDP_FLAGS_DRV_MODE,
            XDP_FLAGS_HW_MODE, XDP_FLAGS_REPLACE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
