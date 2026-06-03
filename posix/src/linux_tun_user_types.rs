//! `<linux/if_tun.h>` — TUN/TAP virtual network devices.
//!
//! A TUN device exposes an L3 interface (raw IP packets) to userspace
//! via a single fd; a TAP device exposes an L2 interface (Ethernet
//! frames). VPNs (OpenVPN, WireGuard userspace), VMs (QEMU/KVM), and
//! container networking all use them.

// ---------------------------------------------------------------------------
// `/dev/net/tun` — the cloning device userspace opens
// ---------------------------------------------------------------------------

pub const DEV_NET_TUN: &str = "/dev/net/tun";

// ---------------------------------------------------------------------------
// `TUNSETIFF` flags — request layer-2 or layer-3 mode plus options
// ---------------------------------------------------------------------------

pub const IFF_TUN: u16 = 0x0001;
pub const IFF_TAP: u16 = 0x0002;
pub const IFF_NAPI: u16 = 0x0010;
pub const IFF_NAPI_FRAGS: u16 = 0x0020;
pub const IFF_NO_PI: u16 = 0x1000;
pub const IFF_ONE_QUEUE: u16 = 0x2000;
pub const IFF_VNET_HDR: u16 = 0x4000;
pub const IFF_TUN_EXCL: u16 = 0x8000;
pub const IFF_MULTI_QUEUE: u16 = 0x0100;
pub const IFF_ATTACH_QUEUE: u16 = 0x0200;
pub const IFF_DETACH_QUEUE: u16 = 0x0400;
pub const IFF_PERSIST: u16 = 0x0800;

// ---------------------------------------------------------------------------
// ioctl numbers (`'T'` magic, sequence 200…)
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
pub const TUNGETSNDBUF: u32 = 0x80045431;
pub const TUNSETSNDBUF: u32 = 0x40045432;

// ---------------------------------------------------------------------------
// virtio-net header offload features (`TUNSETOFFLOAD`)
// ---------------------------------------------------------------------------

pub const TUN_F_CSUM: u32 = 0x01;
pub const TUN_F_TSO4: u32 = 0x02;
pub const TUN_F_TSO6: u32 = 0x04;
pub const TUN_F_TSO_ECN: u32 = 0x08;
pub const TUN_F_UFO: u32 = 0x10;
pub const TUN_F_USO4: u32 = 0x20;
pub const TUN_F_USO6: u32 = 0x40;

// ---------------------------------------------------------------------------
// Packet-info header (4 bytes, prepended when IFF_NO_PI not set)
// ---------------------------------------------------------------------------

pub const TUN_PI_SIZE: usize = 4;
pub const TUN_PKT_STRIP: u16 = 0x0001;

// ---------------------------------------------------------------------------
// Default MTU
// ---------------------------------------------------------------------------

pub const TUN_READQ_SIZE: u32 = 500;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iff_tun_and_tap_disjoint() {
        // Exactly one of TUN/TAP can be picked — they live in adjacent
        // low bits and don't overlap.
        assert_eq!(IFF_TUN & IFF_TAP, 0);
        assert_eq!(IFF_TUN, 1);
        assert_eq!(IFF_TAP, 2);
    }

    #[test]
    fn test_high_byte_iff_flags() {
        // IFF_NO_PI through IFF_TUN_EXCL live in the high byte to keep
        // them clear of the netdevice `IFF_*` flags (UP/RUNNING/etc).
        let h = [
            IFF_NO_PI, IFF_ONE_QUEUE, IFF_VNET_HDR, IFF_TUN_EXCL, IFF_MULTI_QUEUE,
            IFF_ATTACH_QUEUE, IFF_DETACH_QUEUE, IFF_PERSIST,
        ];
        for v in h {
            assert!(v >= 0x0100, "{v:#x} should be in high byte");
            assert!(v.is_power_of_two());
        }
    }

    #[test]
    fn test_tunsetiff_in_t_ioctl_namespace() {
        // 'T' = 0x54 — TUN ioctls live in that magic range.
        // The low 16 bits of the ioctl number encode 'T'<<8 | seq.
        assert_eq!(TUNSETIFF & 0xFFFF, 0x54CA);
        assert_eq!(TUNSETNOCSUM & 0xFFFF, 0x54C8);
        assert_eq!(TUNGETIFF & 0xFFFF, 0x54D2);
        // The setters use the IOC_WRITE direction (top bit 0x40000000),
        // the getters use IOC_READ (0x80000000).
        assert_eq!(TUNSETIFF & 0xC0000000, 0x40000000);
        assert_eq!(TUNGETIFF & 0xC0000000, 0x80000000);
    }

    #[test]
    fn test_tun_offload_flags_low_7_bits() {
        let o = [TUN_F_CSUM, TUN_F_TSO4, TUN_F_TSO6, TUN_F_TSO_ECN, TUN_F_UFO, TUN_F_USO4, TUN_F_USO6];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v, 1 << i);
        }
        let mut or = 0u32;
        for v in o {
            or |= v;
        }
        assert_eq!(or, 0x7F);
    }

    #[test]
    fn test_pi_header_is_4_bytes() {
        // struct tun_pi { __u16 flags; __be16 proto; } — 4 bytes total.
        assert_eq!(TUN_PI_SIZE, 4);
        assert_eq!(TUN_PKT_STRIP, 1);
    }

    #[test]
    fn test_dev_path_and_readq() {
        assert_eq!(DEV_NET_TUN, "/dev/net/tun");
        // Default per-queue read pending: 500 packets (tun.c TUN_READQ_SIZE).
        assert_eq!(TUN_READQ_SIZE, 500);
    }
}
