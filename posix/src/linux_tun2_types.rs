//! `<linux/if_tun.h>` — TUN/TAP device constants (extended).
//!
//! Extended TUN/TAP constants covering IOCTL commands,
//! device flags, feature flags, and filter configuration.

// ---------------------------------------------------------------------------
// TUN/TAP IOCTL commands
// ---------------------------------------------------------------------------

/// Set interface (create/attach).
pub const TUNSETIFF: u32 = 0x400454CA;
/// Set persistent mode.
pub const TUNSETPERSIST: u32 = 0x400454CB;
/// Set owner UID.
pub const TUNSETOWNER: u32 = 0x400454CC;
/// Set owner GID.
pub const TUNSETGROUP: u32 = 0x400454CE;
/// Get features.
pub const TUNGETFEATURES: u32 = 0x800454CF;
/// Set offload.
pub const TUNSETOFFLOAD: u32 = 0x400454D0;
/// Set tx filter.
pub const TUNSETTXFILTER: u32 = 0x400454D1;
/// Get interface.
pub const TUNGETIFF: u32 = 0x800454D2;
/// Get sndbuf.
pub const TUNGETSNDBUF: u32 = 0x800454D3;
/// Set sndbuf.
pub const TUNSETSNDBUF: u32 = 0x400454D4;
/// Attach filter.
pub const TUNATTACHFILTER: u32 = 0x401054D5;
/// Detach filter.
pub const TUNDETACHFILTER: u32 = 0x401054D6;
/// Get vnet header size.
pub const TUNGETVNETLE: u32 = 0x800454DD;
/// Set vnet header size.
pub const TUNSETVNETLE: u32 = 0x400454DC;
/// Get vnet header size be.
pub const TUNGETVNETBE: u32 = 0x800454DF;
/// Set vnet header size be.
pub const TUNSETVNETBE: u32 = 0x400454DE;
/// Set steering eBPF.
pub const TUNSETSTEERINGEBPF: u32 = 0x800454E0;
/// Set filter eBPF.
pub const TUNSETFILTEREBPF: u32 = 0x800454E1;
/// Set carrier.
pub const TUNSETCARRIER: u32 = 0x400454E2;
/// Get dev net namespace.
pub const TUNGETDEVNETNS: u32 = 0x54E3;

// ---------------------------------------------------------------------------
// TUN/TAP interface flags
// ---------------------------------------------------------------------------

/// TUN device (Layer 3, IP packets).
pub const IFF_TUN: u16 = 0x0001;
/// TAP device (Layer 2, Ethernet frames).
pub const IFF_TAP: u16 = 0x0002;
/// Use NAPI (new API) for packet processing.
pub const IFF_NAPI: u16 = 0x0010;
/// Use NAPI fragmented receive.
pub const IFF_NAPI_FRAGS: u16 = 0x0020;
/// No packet info header.
pub const IFF_NO_PI: u16 = 0x1000;
/// One queue (legacy).
pub const IFF_ONE_QUEUE: u16 = 0x2000;
/// Virtio net header in packets.
pub const IFF_VNET_HDR: u16 = 0x4000;
/// Multi-queue support.
pub const IFF_MULTI_QUEUE: u16 = 0x0100;
/// Attach to existing queue.
pub const IFF_ATTACH_QUEUE: u16 = 0x0200;
/// Detach from queue.
pub const IFF_DETACH_QUEUE: u16 = 0x0400;
/// Persist across close.
pub const IFF_PERSIST: u16 = 0x0800;
/// No filter.
pub const IFF_NOFILTER: u16 = 0x1000;

// ---------------------------------------------------------------------------
// TUN offload flags
// ---------------------------------------------------------------------------

/// TCP segmentation offload (v4).
pub const TUN_F_CSUM: u32 = 0x01;
/// TCP segmentation offload (v4).
pub const TUN_F_TSO4: u32 = 0x02;
/// TCP segmentation offload (v6).
pub const TUN_F_TSO6: u32 = 0x04;
/// TCP ECN.
pub const TUN_F_TSO_ECN: u32 = 0x08;
/// UDP fragmentation offload.
pub const TUN_F_UFO: u32 = 0x10;
/// UDP segmentation offload.
pub const TUN_F_USO4: u32 = 0x20;
/// UDP segmentation offload (v6).
pub const TUN_F_USO6: u32 = 0x40;

// ---------------------------------------------------------------------------
// TUN packet info flags
// ---------------------------------------------------------------------------

/// TUN_PKT_STRIP — packet was stripped.
pub const TUN_PKT_STRIP: u32 = 0x0001;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            TUNSETIFF, TUNSETPERSIST, TUNSETOWNER, TUNSETGROUP,
            TUNGETFEATURES, TUNSETOFFLOAD, TUNSETTXFILTER,
            TUNGETIFF, TUNGETSNDBUF, TUNSETSNDBUF,
            TUNATTACHFILTER, TUNDETACHFILTER,
            TUNGETVNETLE, TUNSETVNETLE,
            TUNGETVNETBE, TUNSETVNETBE,
            TUNSETSTEERINGEBPF, TUNSETFILTEREBPF,
            TUNSETCARRIER, TUNGETDEVNETNS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_tun_tap_flags() {
        assert_eq!(IFF_TUN, 0x0001);
        assert_eq!(IFF_TAP, 0x0002);
    }

    #[test]
    fn test_offload_flags_powers_of_two() {
        let flags = [
            TUN_F_CSUM, TUN_F_TSO4, TUN_F_TSO6,
            TUN_F_TSO_ECN, TUN_F_UFO, TUN_F_USO4, TUN_F_USO6,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_offload_flags_no_overlap() {
        let flags = [
            TUN_F_CSUM, TUN_F_TSO4, TUN_F_TSO6,
            TUN_F_TSO_ECN, TUN_F_UFO, TUN_F_USO4, TUN_F_USO6,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_tunsetiff_value() {
        assert_eq!(TUNSETIFF, 0x400454CA);
    }

    #[test]
    fn test_pkt_strip() {
        assert_eq!(TUN_PKT_STRIP, 0x0001);
    }
}
