//! `<linux/if_tun.h>` — TUN/TAP virtual network device ABI.
//!
//! `tun`/`tap` is the universal Linux virtual NIC: every VPN client
//! (OpenVPN, WireGuard, sshuttle), every container network plug-in,
//! every VM running QEMU/KVM, and every `libvirt`-managed bridge ends
//! up calling `TUNSETIFF` on `/dev/net/tun`.

// ---------------------------------------------------------------------------
// /dev path
// ---------------------------------------------------------------------------

/// Path to the TUN/TAP control device.
pub const TUN_DEV_PATH: &str = "/dev/net/tun";

// ---------------------------------------------------------------------------
// TUNSETIFF / struct ifreq.ifr_flags
// ---------------------------------------------------------------------------

/// Layer-3 tunnel (no Ethernet header).
pub const IFF_TUN: u16 = 0x0001;
/// Layer-2 tap (full Ethernet frames).
pub const IFF_TAP: u16 = 0x0002;
/// No persistent allocation (deprecated).
pub const IFF_NO_PI: u16 = 0x1000;
/// One queue per fd (multi-queue otherwise).
pub const IFF_ONE_QUEUE: u16 = 0x2000;
/// `IFF_VNET_HDR` — frames include vnet_hdr.
pub const IFF_VNET_HDR: u16 = 0x4000;
/// Allow tun to be opened by tunnel daemon user.
pub const IFF_TUN_EXCL: u16 = 0x8000;
/// `IFF_MULTI_QUEUE` — open additional queues with TUNSETQUEUE.
pub const IFF_MULTI_QUEUE: u16 = 0x0100;
/// `IFF_ATTACH_QUEUE` — TUNSETQUEUE.
pub const IFF_ATTACH_QUEUE: u16 = 0x0200;
/// `IFF_DETACH_QUEUE` — TUNSETQUEUE.
pub const IFF_DETACH_QUEUE: u16 = 0x0400;
/// `IFF_PERSIST` — keep the interface after fd close.
pub const IFF_PERSIST: u16 = 0x0800;
/// `IFF_NOFILTER` — bypass attached BPF filter on tx.
pub const IFF_NOFILTER: u16 = 0x1000;

// ---------------------------------------------------------------------------
// ioctl numbers (raw values, _IOW('T', n, …))
// ---------------------------------------------------------------------------

pub const TUNSETNOCSUM: u32 = 0x400454C8;
pub const TUNSETDEBUG: u32 = 0x400454C9;
pub const TUNSETIFF: u32 = 0x400454CA;
pub const TUNSETPERSIST: u32 = 0x400454CB;
pub const TUNSETOWNER: u32 = 0x400454CC;
pub const TUNSETLINK: u32 = 0x400454CD;
pub const TUNSETGROUP: u32 = 0x400454CE;
pub const TUNGETFEATURES: u32 = 0x800454CF;
pub const TUNSETOFFLOAD: u32 = 0x400454D0;
pub const TUNSETTXFILTER: u32 = 0x400454D1;
pub const TUNGETIFF: u32 = 0x800454D2;
pub const TUNGETSNDBUF: u32 = 0x800454D3;
pub const TUNSETSNDBUF: u32 = 0x400454D4;
pub const TUNGETVNETHDRSZ: u32 = 0x800454D7;
pub const TUNSETVNETHDRSZ: u32 = 0x400454D8;
pub const TUNSETQUEUE: u32 = 0x400454D9;

// ---------------------------------------------------------------------------
// Protocol-info header (when IFF_NO_PI is *not* set)
// ---------------------------------------------------------------------------

/// `struct tun_pi.flags` — TUN_PKT_STRIP indicates VLAN-stripped frame.
pub const TUN_PKT_STRIP: u16 = 0x0001;

// ---------------------------------------------------------------------------
// TUN feature flags reported by TUNGETFEATURES
// ---------------------------------------------------------------------------

pub const TUN_F_CSUM: u32 = 0x01;
pub const TUN_F_TSO4: u32 = 0x02;
pub const TUN_F_TSO6: u32 = 0x04;
pub const TUN_F_TSO_ECN: u32 = 0x08;
pub const TUN_F_UFO: u32 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_path() {
        // udev creates /dev/net/tun unconditionally.
        assert_eq!(TUN_DEV_PATH, "/dev/net/tun");
    }

    #[test]
    fn test_iff_tun_tap_distinct() {
        assert_ne!(IFF_TUN, IFF_TAP);
        assert!(IFF_TUN.is_power_of_two());
        assert!(IFF_TAP.is_power_of_two());
    }

    #[test]
    fn test_iff_modifier_bits_pow2() {
        for &b in &[
            IFF_NO_PI,
            IFF_ONE_QUEUE,
            IFF_VNET_HDR,
            IFF_TUN_EXCL,
            IFF_MULTI_QUEUE,
            IFF_ATTACH_QUEUE,
            IFF_DETACH_QUEUE,
            IFF_PERSIST,
        ] {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_ioctls_use_T_magic() {
        // _IO* macros put the 'T' (0x54) ioctl magic in bits 8..15.
        for &io in &[
            TUNSETIFF,
            TUNSETPERSIST,
            TUNGETIFF,
            TUNGETFEATURES,
            TUNSETQUEUE,
        ] {
            assert_eq!((io >> 8) & 0xFF, 0x54);
        }
    }

    #[test]
    fn test_tun_features_pow2() {
        for &b in &[TUN_F_CSUM, TUN_F_TSO4, TUN_F_TSO6, TUN_F_TSO_ECN, TUN_F_UFO] {
            assert!(b.is_power_of_two());
        }
        // OR of all five fits in a byte.
        let all = TUN_F_CSUM | TUN_F_TSO4 | TUN_F_TSO6 | TUN_F_TSO_ECN | TUN_F_UFO;
        assert_eq!(all & 0xFF, all);
    }

    #[test]
    fn test_pkt_strip_constant() {
        assert_eq!(TUN_PKT_STRIP, 1);
    }
}
