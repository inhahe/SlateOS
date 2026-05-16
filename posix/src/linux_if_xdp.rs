//! `<linux/if_xdp.h>` — AF_XDP (eXpress Data Path) socket constants.
//!
//! AF_XDP provides a high-performance raw packet I/O path that
//! bypasses most of the kernel networking stack. Used for high-speed
//! packet processing, firewalls, and custom protocol implementations.

// ---------------------------------------------------------------------------
// Socket options
// ---------------------------------------------------------------------------

/// Receive ring (SOL_XDP level).
pub const XDP_RX_RING: i32 = 1;
/// Transmit ring.
pub const XDP_TX_RING: i32 = 2;
/// UMEM registration.
pub const XDP_UMEM_REG: i32 = 3;
/// UMEM fill ring.
pub const XDP_UMEM_FILL_RING: i32 = 4;
/// UMEM completion ring.
pub const XDP_UMEM_COMPLETION_RING: i32 = 5;
/// Get statistics.
pub const XDP_STATISTICS: i32 = 7;
/// Get mmap offsets.
pub const XDP_MMAP_OFFSETS: i32 = 1;
/// Get options.
pub const XDP_OPTIONS: i32 = 8;

// ---------------------------------------------------------------------------
// Bind flags
// ---------------------------------------------------------------------------

/// Share UMEM between sockets.
pub const XDP_SHARED_UMEM: u16 = 1 << 0;
/// Copy mode (slower but more compatible).
pub const XDP_COPY: u16 = 1 << 1;
/// Zero-copy mode.
pub const XDP_ZEROCOPY: u16 = 1 << 2;
/// Use need-wakeup optimization.
pub const XDP_USE_NEED_WAKEUP: u16 = 1 << 3;

// ---------------------------------------------------------------------------
// Ring flags
// ---------------------------------------------------------------------------

/// Need wakeup flag (ring).
pub const XDP_RING_NEED_WAKEUP: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// UMEM flags
// ---------------------------------------------------------------------------

/// Unaligned chunk mode.
pub const XDP_UMEM_UNALIGNED_CHUNK_FLAG: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// XDP verdict/action (from bpf.h, commonly used with AF_XDP)
// ---------------------------------------------------------------------------

/// Drop the packet.
pub const XDP_ABORTED: u32 = 0;
/// Drop (same as aborted but distinct).
pub const XDP_DROP: u32 = 1;
/// Pass to normal network stack.
pub const XDP_PASS: u32 = 2;
/// Transmit out same interface.
pub const XDP_TX: u32 = 3;
/// Redirect to another interface/socket.
pub const XDP_REDIRECT: u32 = 4;

// ---------------------------------------------------------------------------
// Mmap offset pages
// ---------------------------------------------------------------------------

/// RX ring mmap offset.
pub const XDP_PGOFF_RX_RING: u64 = 0;
/// TX ring mmap offset.
pub const XDP_PGOFF_TX_RING: u64 = 0x80000000;
/// Fill ring mmap offset.
pub const XDP_UMEM_PGOFF_FILL_RING: u64 = 0x100000000;
/// Completion ring mmap offset.
pub const XDP_UMEM_PGOFF_COMPLETION_RING: u64 = 0x180000000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_flags_powers_of_two() {
        let flags = [
            XDP_SHARED_UMEM, XDP_COPY, XDP_ZEROCOPY,
            XDP_USE_NEED_WAKEUP,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} not power of 2");
        }
    }

    #[test]
    fn test_verdicts_sequential() {
        assert_eq!(XDP_ABORTED, 0);
        assert_eq!(XDP_DROP, 1);
        assert_eq!(XDP_PASS, 2);
        assert_eq!(XDP_TX, 3);
        assert_eq!(XDP_REDIRECT, 4);
    }

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            XDP_RX_RING, XDP_TX_RING, XDP_UMEM_REG,
            XDP_UMEM_FILL_RING, XDP_UMEM_COMPLETION_RING,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_mmap_offsets_distinct() {
        let offsets = [
            XDP_PGOFF_RX_RING, XDP_PGOFF_TX_RING,
            XDP_UMEM_PGOFF_FILL_RING, XDP_UMEM_PGOFF_COMPLETION_RING,
        ];
        for i in 0..offsets.len() {
            for j in (i + 1)..offsets.len() {
                assert_ne!(offsets[i], offsets[j]);
            }
        }
    }
}
