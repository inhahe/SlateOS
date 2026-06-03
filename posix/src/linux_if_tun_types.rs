//! `<linux/if_tun.h>` — TUN/TAP virtual network device constants.
//!
//! TUN/TAP devices create virtual network interfaces in software.
//! TUN operates at layer 3 (IP packets), TAP at layer 2 (Ethernet
//! frames). Used by VPNs (OpenVPN, WireGuard userspace), virtual
//! machines (QEMU), containers, and network testing tools.
//! Created via /dev/net/tun with ioctl configuration.

// ---------------------------------------------------------------------------
// TUN/TAP ioctl commands
// ---------------------------------------------------------------------------

/// Set interface parameters (create/configure).
pub const TUNSETIFF: u32 = 0x400454CA;
/// Set persistent mode.
pub const TUNSETPERSIST: u32 = 0x400454CB;
/// Set owner UID.
pub const TUNSETOWNER: u32 = 0x400454CC;
/// Set group GID.
pub const TUNSETGROUP: u32 = 0x400454CE;
/// Get interface features.
pub const TUNGETFEATURES: u32 = 0x800454CF;
/// Set offload features.
pub const TUNSETOFFLOAD: u32 = 0x400454D0;
/// Set transmit queue length.
pub const TUNSETQUEUE: u32 = 0x400454D9;
/// Get sndbuf size.
pub const TUNGETSNDBUF: u32 = 0x800454D3;
/// Set sndbuf size.
pub const TUNSETSNDBUF: u32 = 0x400454D4;
/// Attach filter (BPF program).
pub const TUNSETTXFILTER: u32 = 0x400454D1;
/// Get interface index.
pub const TUNGETIFF: u32 = 0x800454D2;
/// Set vnet header size.
pub const TUNSETVNETHDRSZ: u32 = 0x400454D8;
/// Get vnet header size.
pub const TUNGETVNETHDRSZ: u32 = 0x800454D7;

// ---------------------------------------------------------------------------
// Interface flags (for TUNSETIFF)
// ---------------------------------------------------------------------------

/// TUN device (layer 3, IP packets).
pub const IFF_TUN: u16 = 0x0001;
/// TAP device (layer 2, Ethernet frames).
pub const IFF_TAP: u16 = 0x0002;
/// No protocol info prepended (no PI header).
pub const IFF_NO_PI: u16 = 0x1000;
/// One queue per fd (multi-queue).
pub const IFF_ONE_QUEUE: u16 = 0x2000;
/// Virtio net header support.
pub const IFF_VNET_HDR: u16 = 0x4000;
/// Multi-queue support.
pub const IFF_MULTI_QUEUE: u16 = 0x0100;

// ---------------------------------------------------------------------------
// Feature flags (TUNGETFEATURES result)
// ---------------------------------------------------------------------------

/// Supports IFF_TUN.
pub const TUN_F_TUN: u32 = 0x0001;
/// Supports IFF_TAP.
pub const TUN_F_TAP: u32 = 0x0002;
/// Supports IFF_NO_PI.
pub const TUN_F_NO_PI: u32 = 0x1000;
/// Supports IFF_ONE_QUEUE.
pub const TUN_F_ONE_QUEUE: u32 = 0x2000;
/// Supports IFF_VNET_HDR.
pub const TUN_F_VNET_HDR: u32 = 0x4000;

// ---------------------------------------------------------------------------
// Offload flags (TUNSETOFFLOAD)
// ---------------------------------------------------------------------------

/// TCP segmentation offload (checksum).
pub const TUN_F_CSUM: u32 = 0x01;
/// TSO for IPv4.
pub const TUN_F_TSO4: u32 = 0x02;
/// TSO for IPv6.
pub const TUN_F_TSO6: u32 = 0x04;
/// TCP ECN support.
pub const TUN_F_TSO_ECN: u32 = 0x08;
/// UDP fragmentation offload.
pub const TUN_F_UFO: u32 = 0x10;
/// UDP segmentation offload.
pub const TUN_F_USO4: u32 = 0x20;
/// UDP segmentation offload (IPv6).
pub const TUN_F_USO6: u32 = 0x40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            TUNSETIFF,
            TUNSETPERSIST,
            TUNSETOWNER,
            TUNSETGROUP,
            TUNGETFEATURES,
            TUNSETOFFLOAD,
            TUNSETQUEUE,
            TUNGETSNDBUF,
            TUNSETSNDBUF,
            TUNGETIFF,
            TUNSETVNETHDRSZ,
            TUNGETVNETHDRSZ,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_tun_tap_distinct() {
        assert_ne!(IFF_TUN, IFF_TAP);
        assert_eq!(IFF_TUN & IFF_TAP, 0);
    }

    #[test]
    fn test_offload_flags_no_overlap() {
        let flags = [
            TUN_F_CSUM,
            TUN_F_TSO4,
            TUN_F_TSO6,
            TUN_F_TSO_ECN,
            TUN_F_UFO,
            TUN_F_USO4,
            TUN_F_USO6,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_iff_flags_no_pi() {
        // NO_PI should not conflict with TUN/TAP.
        assert_eq!(IFF_NO_PI & IFF_TUN, 0);
        assert_eq!(IFF_NO_PI & IFF_TAP, 0);
    }
}
