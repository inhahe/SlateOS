//! `<linux/if_tun.h>` — TUN/TAP virtual network device constants.
//!
//! TUN (layer 3) and TAP (layer 2) devices allow userspace
//! programs to send and receive network packets. Used by VPNs
//! (OpenVPN, WireGuard userspace), virtual machines (QEMU),
//! and network namespaces.

// ---------------------------------------------------------------------------
// Device file
// ---------------------------------------------------------------------------

/// TUN/TAP control device.
pub const TUN_DEV_PATH: &str = "/dev/net/tun";

// ---------------------------------------------------------------------------
// ioctl commands
// ---------------------------------------------------------------------------

/// Set interface parameters.
pub const TUNSETIFF: u32 = 0x400454CA;
/// Set persistent mode.
pub const TUNSETPERSIST: u32 = 0x400454CB;
/// Set owner.
pub const TUNSETOWNER: u32 = 0x400454CC;
/// Set group.
pub const TUNSETGROUP: u32 = 0x400454CE;
/// Get features.
pub const TUNGETFEATURES: u32 = 0x800454CF;
/// Set offload.
pub const TUNSETOFFLOAD: u32 = 0x400454D0;
/// Set interface queue.
pub const TUNSETQUEUE: u32 = 0x400454D9;
/// Get sndbuf.
pub const TUNGETSNDBUF: u32 = 0x800454D3;
/// Set sndbuf.
pub const TUNSETSNDBUF: u32 = 0x400454D4;

// ---------------------------------------------------------------------------
// Interface flags (TUNSETIFF)
// ---------------------------------------------------------------------------

/// TUN device (layer 3, IP packets).
pub const IFF_TUN: u16 = 0x0001;
/// TAP device (layer 2, Ethernet frames).
pub const IFF_TAP: u16 = 0x0002;
/// No protocol info prepended.
pub const IFF_NO_PI: u16 = 0x1000;
/// One queue mode.
pub const IFF_ONE_QUEUE: u16 = 0x2000;
/// Virtio net header support.
pub const IFF_VNET_HDR: u16 = 0x4000;
/// Multi-queue support.
pub const IFF_MULTI_QUEUE: u16 = 0x0100;

// ---------------------------------------------------------------------------
// Queue flags (TUNSETQUEUE)
// ---------------------------------------------------------------------------

/// Attach queue.
pub const IFF_ATTACH_QUEUE: u16 = 0x0200;
/// Detach queue.
pub const IFF_DETACH_QUEUE: u16 = 0x0400;

// ---------------------------------------------------------------------------
// Offload features
// ---------------------------------------------------------------------------

/// TCP segmentation offload.
pub const TUN_F_TSO4: u32 = 0x01;
/// TCPv6 segmentation offload.
pub const TUN_F_TSO6: u32 = 0x02;
/// TCP ECN.
pub const TUN_F_TSO_ECN: u32 = 0x04;
/// UDP fragmentation offload.
pub const TUN_F_UFO: u32 = 0x08;
/// Checksum offload.
pub const TUN_F_CSUM: u32 = 0x10;
/// UDP segmentation offload.
pub const TUN_F_USO4: u32 = 0x20;
/// UDPv6 segmentation offload.
pub const TUN_F_USO6: u32 = 0x40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            TUNSETIFF,
            TUNSETPERSIST,
            TUNSETOWNER,
            TUNSETGROUP,
            TUNGETFEATURES,
            TUNSETOFFLOAD,
            TUNSETQUEUE,
            TUNGETSNDBUF,
            TUNSETSNDBUF,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_iff_tun_tap_distinct() {
        assert_ne!(IFF_TUN, IFF_TAP);
        assert_eq!(IFF_TUN & IFF_TAP, 0);
    }

    #[test]
    fn test_iff_flags_no_overlap() {
        let flags = [
            IFF_TUN,
            IFF_TAP,
            IFF_NO_PI,
            IFF_ONE_QUEUE,
            IFF_VNET_HDR,
            IFF_MULTI_QUEUE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(
                    flags[i] & flags[j],
                    0,
                    "0x{:x} & 0x{:x}",
                    flags[i],
                    flags[j]
                );
            }
        }
    }

    #[test]
    fn test_queue_flags_distinct() {
        assert_ne!(IFF_ATTACH_QUEUE, IFF_DETACH_QUEUE);
        assert_eq!(IFF_ATTACH_QUEUE & IFF_DETACH_QUEUE, 0);
    }

    #[test]
    fn test_offload_flags_powers_of_two() {
        let flags = [
            TUN_F_TSO4,
            TUN_F_TSO6,
            TUN_F_TSO_ECN,
            TUN_F_UFO,
            TUN_F_CSUM,
            TUN_F_USO4,
            TUN_F_USO6,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }
}
