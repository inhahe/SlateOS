//! `<linux/ip_vs.h>` — IP Virtual Server (IPVS) constants.
//!
//! IPVS is a layer-4 load balancer built into the Linux kernel.
//! Used by LVS (Linux Virtual Server), kube-proxy (in IPVS mode),
//! and keepalived for high-availability service clusters.

// ---------------------------------------------------------------------------
// IPVS forwarding methods
// ---------------------------------------------------------------------------

/// NAT (masquerading) forwarding.
pub const IP_VS_CONN_F_MASQ: u32 = 0x0000;
/// Direct routing (gateway).
pub const IP_VS_CONN_F_LOCALNODE: u32 = 0x0001;
/// IP tunneling (IPIP).
pub const IP_VS_CONN_F_TUNNEL: u32 = 0x0002;
/// Direct routing (DR).
pub const IP_VS_CONN_F_DROUTE: u32 = 0x0003;
/// Bypass (no load balancing).
pub const IP_VS_CONN_F_BYPASS: u32 = 0x0004;

/// Forwarding method mask.
pub const IP_VS_CONN_F_FWD_MASK: u32 = 0x0007;

// ---------------------------------------------------------------------------
// IPVS scheduler algorithms
// ---------------------------------------------------------------------------

/// Round-robin.
pub const IP_VS_SCHED_RR: &str = "rr";
/// Weighted round-robin.
pub const IP_VS_SCHED_WRR: &str = "wrr";
/// Least connection.
pub const IP_VS_SCHED_LC: &str = "lc";
/// Weighted least connection.
pub const IP_VS_SCHED_WLC: &str = "wlc";
/// Shortest expected delay.
pub const IP_VS_SCHED_SED: &str = "sed";
/// Never queue.
pub const IP_VS_SCHED_NQ: &str = "nq";
/// Source hashing.
pub const IP_VS_SCHED_SH: &str = "sh";
/// Destination hashing.
pub const IP_VS_SCHED_DH: &str = "dh";

// ---------------------------------------------------------------------------
// Generic Netlink commands
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IPVS_CMD_UNSPEC: u8 = 0;
/// New service.
pub const IPVS_CMD_NEW_SERVICE: u8 = 1;
/// Set service.
pub const IPVS_CMD_SET_SERVICE: u8 = 2;
/// Delete service.
pub const IPVS_CMD_DEL_SERVICE: u8 = 3;
/// Get service.
pub const IPVS_CMD_GET_SERVICE: u8 = 4;
/// New destination.
pub const IPVS_CMD_NEW_DEST: u8 = 5;
/// Set destination.
pub const IPVS_CMD_SET_DEST: u8 = 6;
/// Delete destination.
pub const IPVS_CMD_DEL_DEST: u8 = 7;
/// Get destination.
pub const IPVS_CMD_GET_DEST: u8 = 8;
/// New daemon.
pub const IPVS_CMD_NEW_DAEMON: u8 = 9;
/// Delete daemon.
pub const IPVS_CMD_DEL_DAEMON: u8 = 10;
/// Get daemon.
pub const IPVS_CMD_GET_DAEMON: u8 = 11;
/// Flush (delete all).
pub const IPVS_CMD_FLUSH: u8 = 14;
/// Zero counters.
pub const IPVS_CMD_ZERO: u8 = 15;
/// Get info.
pub const IPVS_CMD_GET_INFO: u8 = 16;

// ---------------------------------------------------------------------------
// Connection flags
// ---------------------------------------------------------------------------

/// One-packet scheduling.
pub const IP_VS_CONN_F_ONE_PACKET: u32 = 0x0020;
/// Persistent connection.
pub const IP_VS_CONN_F_PERSISTENT: u32 = 0x0040;
/// Hashed entry.
pub const IP_VS_CONN_F_HASHED: u32 = 0x0100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fwd_methods_distinct() {
        let methods = [
            IP_VS_CONN_F_MASQ, IP_VS_CONN_F_LOCALNODE,
            IP_VS_CONN_F_TUNNEL, IP_VS_CONN_F_DROUTE,
            IP_VS_CONN_F_BYPASS,
        ];
        for i in 0..methods.len() {
            for j in (i + 1)..methods.len() {
                assert_ne!(methods[i], methods[j]);
            }
        }
    }

    #[test]
    fn test_schedulers() {
        assert_eq!(IP_VS_SCHED_RR, "rr");
        assert_eq!(IP_VS_SCHED_WLC, "wlc");
    }

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            IPVS_CMD_UNSPEC, IPVS_CMD_NEW_SERVICE, IPVS_CMD_SET_SERVICE,
            IPVS_CMD_DEL_SERVICE, IPVS_CMD_GET_SERVICE,
            IPVS_CMD_NEW_DEST, IPVS_CMD_SET_DEST,
            IPVS_CMD_DEL_DEST, IPVS_CMD_GET_DEST,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_fwd_mask() {
        // All forwarding methods should be within the mask.
        assert_eq!(IP_VS_CONN_F_MASQ & IP_VS_CONN_F_FWD_MASK, IP_VS_CONN_F_MASQ);
        assert_eq!(IP_VS_CONN_F_TUNNEL & IP_VS_CONN_F_FWD_MASK, IP_VS_CONN_F_TUNNEL);
        assert_eq!(IP_VS_CONN_F_DROUTE & IP_VS_CONN_F_FWD_MASK, IP_VS_CONN_F_DROUTE);
    }
}
