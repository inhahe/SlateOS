//! `<linux/if_tun.h>` — TUN/TAP device definitions.
//!
//! Provides ioctl constants and flags for creating and managing
//! TUN (network layer) and TAP (data link layer) virtual interfaces.

// ---------------------------------------------------------------------------
// Ioctl commands
// ---------------------------------------------------------------------------

/// Set TUN/TAP interface parameters.
pub const TUNSETIFF: u64 = 0x4004_54CA;
/// Set persistent mode.
pub const TUNSETPERSIST: u64 = 0x4004_54CB;
/// Set owner.
pub const TUNSETOWNER: u64 = 0x4004_54CC;
/// Set group.
pub const TUNSETGROUP: u64 = 0x4004_54CE;
/// Get features.
pub const TUNGETFEATURES: u64 = 0x8004_54CF;
/// Set offload flags.
pub const TUNSETOFFLOAD: u64 = 0x4004_54D0;
/// Set transmit queue length.
pub const TUNSETQUEUE: u64 = 0x4004_54D9;
/// Get filter.
pub const TUNGETFILTER: u64 = 0x8004_54DB;
/// Set VNET header size.
pub const TUNSETVNETHDRSZ: u64 = 0x4004_54D8;
/// Get VNET header size.
pub const TUNGETVNETHDRSZ: u64 = 0x8004_54D7;
/// Set sndbuf.
pub const TUNSETSNDBUF: u64 = 0x4004_54D4;
/// Attach or detach a queue.
pub const TUNSETIFINDEX: u64 = 0x4004_54DA;

// ---------------------------------------------------------------------------
// Interface flags (for TUNSETIFF)
// ---------------------------------------------------------------------------

/// TUN device (no Ethernet headers).
pub const IFF_TUN: i16 = 0x0001;
/// TAP device (with Ethernet headers).
pub const IFF_TAP: i16 = 0x0002;
/// No packet information header.
pub const IFF_NO_PI: i16 = 0x1000;
/// Don't add flow control header.
pub const IFF_ONE_QUEUE: i16 = 0x2000;
/// VNET header support.
pub const IFF_VNET_HDR: i16 = 0x4000;
/// Multi-queue support.
pub const IFF_MULTI_QUEUE: i16 = 0x0100;
/// Attach a queue.
pub const IFF_ATTACH_QUEUE: i16 = 0x0200;
/// Detach a queue.
pub const IFF_DETACH_QUEUE: i16 = 0x0400;
/// Persist flag.
pub const IFF_PERSIST: i16 = 0x0800;
/// No filter.
pub const IFF_NOFILTER: i16 = 0x1000; // Same as IFF_NO_PI — context-dependent.

// ---------------------------------------------------------------------------
// TUN feature bits
// ---------------------------------------------------------------------------

/// Device supports checksum offload.
pub const TUN_F_CSUM: u32 = 0x01;
/// Device supports TSO for IPv4.
pub const TUN_F_TSO4: u32 = 0x02;
/// Device supports TSO for IPv6.
pub const TUN_F_TSO6: u32 = 0x04;
/// Device supports TCP ECN.
pub const TUN_F_TSO_ECN: u32 = 0x08;
/// Device supports UFO (deprecated).
pub const TUN_F_UFO: u32 = 0x10;

// ---------------------------------------------------------------------------
// TUN packet info header
// ---------------------------------------------------------------------------

/// TUN packet info header (prepended to packets when `IFF_NO_PI` is not set).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TunPi {
    /// Flags.
    pub flags: u16,
    /// Protocol (network byte order).
    pub proto: u16,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tun_tap_flags() {
        assert_eq!(IFF_TUN, 0x0001);
        assert_eq!(IFF_TAP, 0x0002);
        assert_ne!(IFF_TUN, IFF_TAP);
    }

    #[test]
    fn test_ioctl_constants_distinct() {
        let cmds = [
            TUNSETIFF,
            TUNSETPERSIST,
            TUNSETOWNER,
            TUNSETGROUP,
            TUNGETFEATURES,
            TUNSETOFFLOAD,
            TUNSETQUEUE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_tun_features_are_bits() {
        assert_eq!(TUN_F_CSUM, 1);
        assert_eq!(TUN_F_TSO4, 2);
        assert_eq!(TUN_F_TSO6, 4);
        // Should be combinable.
        let combined = TUN_F_CSUM | TUN_F_TSO4 | TUN_F_TSO6;
        assert_eq!(combined, 7);
    }

    #[test]
    fn test_tun_pi_size() {
        assert_eq!(core::mem::size_of::<TunPi>(), 4);
    }

    #[test]
    fn test_iff_no_pi() {
        // IFF_NO_PI should be a flag that can combine with IFF_TUN or IFF_TAP.
        let flags = IFF_TUN | IFF_NO_PI;
        assert_ne!(flags, IFF_TUN);
        assert_ne!(flags, IFF_NO_PI);
    }
}
